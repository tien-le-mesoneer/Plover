//! Single-use launch codes. Every URL the app opens itself (the desktop window, Open in
//! Browser, `plv serve`'s auto-open) carries `#code=<code>` instead of the session
//! token, so the token never lands in browser history; the page trades the code for the
//! token once via `POST /api/session/exchange`.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;

/// How long a minted code stays valid.
pub const CODE_TTL: Duration = Duration::from_secs(60);
/// Failed exchanges allowed per `FAILURE_WINDOW` before every exchange is refused.
const MAX_FAILURES: usize = 20;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub enum RedeemError {
    /// Unknown, used or expired.
    Invalid,
    /// Too many failed attempts recently.
    Limited,
}

pub struct SessionCodes {
    ttl: Duration,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    codes: Vec<(String, Instant)>,
    failures: VecDeque<Instant>,
}

impl SessionCodes {
    pub fn new(ttl: Duration) -> SessionCodes {
        SessionCodes {
            ttl,
            inner: Mutex::default(),
        }
    }

    /// A fresh random code, valid once within the TTL.
    pub fn mint(&self) -> String {
        let code = crate::security::new_token();
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        inner.codes.retain(|(_, expires)| *expires > now);
        inner.codes.push((code.clone(), now + self.ttl));
        code
    }

    /// Consumes `code` if it is valid; compares in constant time against every live code.
    pub fn redeem(&self, code: &str) -> Result<(), RedeemError> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        while inner
            .failures
            .front()
            .is_some_and(|t| now.duration_since(*t) > FAILURE_WINDOW)
        {
            inner.failures.pop_front();
        }
        if inner.failures.len() >= MAX_FAILURES {
            return Err(RedeemError::Limited);
        }
        inner.codes.retain(|(_, expires)| *expires > now);
        let found = inner
            .codes
            .iter()
            .position(|(live, _)| bool::from(live.as_bytes().ct_eq(code.as_bytes())));
        match found {
            Some(i) => {
                inner.codes.remove(i);
                Ok(())
            }
            None => {
                inner.failures.push_back(now);
                Err(RedeemError::Invalid)
            }
        }
    }
}

impl Default for SessionCodes {
    fn default() -> SessionCodes {
        SessionCodes::new(CODE_TTL)
    }
}
