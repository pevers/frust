use mio::{Ready, Registration, Poll, PollOpt, Token};
use notify::{Watcher, RecursiveMode, watcher, DebouncedEvent::Remove, DebouncedEvent::Rename};
use mio::event::Evented;

use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;
use std::io;

pub struct ConfigListener {
  registration: Registration
}

impl ConfigListener {
  pub fn new(path: &'static str) -> ConfigListener {
    let (registration, set_readiness) = Registration::new2();
    
    thread::spawn(move || {
      let (tx, rx) = channel();
      let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();

      loop {
        watcher.watch(path, RecursiveMode::Recursive).unwrap();
        match rx.recv() {
          // Ignore removal and renames
          Ok(Remove(_)) => {},
          Ok(Rename(_,_)) => {},
          Ok(_) => {
            set_readiness.set_readiness(Ready::readable()).unwrap();
          },
          Err(e) => eprint!("Error watching file {:?}", e)
        }
      }
    });

    ConfigListener {
      registration: registration
    }
  }
}

impl Evented for ConfigListener {
  fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
  -> io::Result<()>
  {
    self.registration.register(poll, token, interest, opts)
  }

  fn reregister(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
    -> io::Result<()>
  {
    self.registration.reregister(poll, token, interest, opts)
  }

  fn deregister(&self, poll: &Poll) -> io::Result<()> {
    self.registration.deregister(poll)
  }  
}