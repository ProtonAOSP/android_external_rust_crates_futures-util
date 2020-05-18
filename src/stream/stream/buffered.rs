use crate::stream::{Fuse, FuturesOrdered, StreamExt};
use futures_core::future::Future;
use futures_core::stream::Stream;
use futures_core::task::{Context, Poll};
#[cfg(feature = "sink")]
use futures_sink::Sink;
use pin_project::{pin_project, project};
use core::fmt;
use core::pin::Pin;

/// Stream for the [`buffered`](super::StreamExt::buffered) method.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct Buffered<St>
where
    St: Stream,
    St::Item: Future,
{
    #[pin]
    stream: Fuse<St>,
    in_progress_queue: FuturesOrdered<St::Item>,
    max: usize,
}

impl<St> fmt::Debug for Buffered<St>
where
    St: Stream + fmt::Debug,
    St::Item: Future,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffered")
            .field("stream", &self.stream)
            .field("in_progress_queue", &self.in_progress_queue)
            .field("max", &self.max)
            .finish()
    }
}

impl<St> Buffered<St>
where
    St: Stream,
    St::Item: Future,
{
    pub(super) fn new(stream: St, n: usize) -> Buffered<St> {
        Buffered {
            stream: super::Fuse::new(stream),
            in_progress_queue: FuturesOrdered::new(),
            max: n,
        }
    }

    delegate_access_inner!(stream, St, (.));
}

impl<St> Stream for Buffered<St>
where
    St: Stream,
    St::Item: Future,
{
    type Item = <St::Item as Future>::Output;

    #[project]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        #[project]
        let Buffered { mut stream, in_progress_queue, max } = self.project();

        // First up, try to spawn off as many futures as possible by filling up
        // our queue of futures.
        while in_progress_queue.len() < *max {
            match stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(fut)) => in_progress_queue.push(fut),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        // Attempt to pull the next value from the in_progress_queue
        let res = in_progress_queue.poll_next_unpin(cx);
        if let Some(val) = ready!(res) {
            return Poll::Ready(Some(val))
        }

        // If more values are still coming from the stream, we're not done yet
        if stream.is_done() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let queue_len = self.in_progress_queue.len();
        let (lower, upper) = self.stream.size_hint();
        let lower = lower.saturating_add(queue_len);
        let upper = match upper {
            Some(x) => x.checked_add(queue_len),
            None => None,
        };
        (lower, upper)
    }
}

// Forwarding impl of Sink from the underlying stream
#[cfg(feature = "sink")]
impl<S, Item> Sink<Item> for Buffered<S>
where
    S: Stream + Sink<Item>,
    S::Item: Future,
{
    type Error = S::Error;

    delegate_sink!(stream, Item);
}
