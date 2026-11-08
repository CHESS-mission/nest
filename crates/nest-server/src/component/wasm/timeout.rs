use std::{cmp::Reverse, collections::BinaryHeap, thread::{self, JoinHandle}, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

pub enum TimeoutRequest {
    SetTimer {
        id: u32,
        deadline: Instant,
    },
    Stop,
}

pub struct Timeout {
    pub id: u32,
}

/// A handle to the timeout scheduler.
#[derive(Debug)]
pub struct TimeoutSchedulerHandle {
    /// Receives timeouts.
    pub timeout_rx: Receiver<Timeout>,
    /// Sends timeout requests.
    pub timeout_request_tx: Sender<TimeoutRequest>,

    handle: JoinHandle<()>,
}

impl TimeoutSchedulerHandle {
    /// Stop and join thread.
    pub fn stop_and_join(self) {
        let _ = self.timeout_request_tx.send(TimeoutRequest::Stop);
        let _ = self.handle.join();
    }
}

pub fn start_timeout_scheduler() -> TimeoutSchedulerHandle {
    let (tx_to_scheduler, rx_to_scheduler) = unbounded::<TimeoutRequest>();
    let (tx_from_scheduler, rx_from_scheduler) = unbounded::<Timeout>();

    let handle = thread::spawn(move || {
        let mut timers = BinaryHeap::<Reverse<(Instant, u32)>>::new();

        loop {
            // Determine how long to wait
            let timeout = timers
                .peek()
                .map(|Reverse((deadline, _))| {
                    deadline.saturating_duration_since(Instant::now())
                });

            let recv_result = match timeout {
                Some(dur) => rx_to_scheduler.recv_timeout(dur),
                None => rx_to_scheduler.recv().map_err(|_| {
                    crossbeam_channel::RecvTimeoutError::Disconnected
                }),
            };

            match recv_result {
                Ok(TimeoutRequest::SetTimer { id, deadline }) => {
                    timers.push(Reverse((deadline, id)));
                }


                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // A timer expired
                    if let Some(Reverse((deadline, id))) = timers.peek().cloned()
                        && deadline <= Instant::now()
                    {
                        timers.pop();
                        let _ = tx_from_scheduler.send(Timeout { id });
                    }
                }

                Ok(TimeoutRequest::Stop) => break,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    TimeoutSchedulerHandle {
        timeout_rx: rx_from_scheduler,
        timeout_request_tx: tx_to_scheduler,
        handle,
    }
}

