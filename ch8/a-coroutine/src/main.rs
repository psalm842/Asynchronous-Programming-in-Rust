use std::{time::{Instant, Duration}, thread};

mod future;
mod http;

use crate::http::Http;
use future::{Future, PollState};

// coro fn async_main() {
//     println!("Program starting")
//     let txt = Http::get("/1000/HelloWorld").wait;
//     println!("{txt}");
//     let txt2 = Http::("500/HelloWorld2").wait;
//     println!("{txt2}");
// }

struct Coroutine {
    state: State,
}

enum State {
    Start,
    Wait1(Box<dyn Future<Output = String>>),
    Wait2(Box<dyn Future<Output = String>>),
    Resolved,
}

impl Coroutine {
    fn new() -> Self {
        Self {
            state: State::Start,
        }
    }
}

impl Future for Coroutine {
    type Output = ();

    fn poll(&mut self) -> PollState<Self::Output> {
        match self.state {
            State::Start => {
                println!("Program starting");
                let fut = Box::new(Http::get("/600/HelloWorld1"));
                self.state = State::Wait1(fut);
                PollState::NotReady
            }

            State::Wait1(ref mut fut) => match fut.poll() {
                PollState::Ready(txt) => {
                    println!("{txt}");
                    let fut2 = Box::new(Http::get("/400/HelloWorld2"));
                    self.state = State::Wait2(fut2);
                    PollState::NotReady
                }

                PollState::NotReady => PollState::NotReady,
            },

            State::Wait2(ref mut fut2) => match fut2.poll() {
                PollState::Ready(txt2) => {
                    println!("{txt2}");
                    self.state = State::Resolved;
                    PollState::Ready(())
                }

                PollState::NotReady => PollState::NotReady,
            },

            State::Resolved => panic!("Polled a resolved future"),
        }
    }
}

fn async_main() -> impl Future<Output = ()> {
    Coroutine::new()
}

fn main() {
    let mut future = async_main();

    loop {
        match future.poll() {
            PollState::NotReady => {
                // we could do other work here or schedule another task since this returned `NotReady`
                println!("Polled");
            },
            PollState::Ready(_) => break,
        }
        
        // Since we print every poll, slow down the loop
        thread::sleep(Duration::from_millis(100));
    }
}