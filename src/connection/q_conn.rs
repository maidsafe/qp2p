// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use std::ops::{Deref, DerefMut};

/// A quic-connection wrapper that will destroy the connection on drop
pub struct QConn {
    q_conn: quinn::Connection,
}

impl From<quinn::Connection> for QConn {
    fn from(q_conn: quinn::Connection) -> Self {
        Self { q_conn }
    }
}

impl Deref for QConn {
    type Target = quinn::Connection;

    fn deref(&self) -> &Self::Target {
        &self.q_conn
    }
}

impl DerefMut for QConn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.q_conn
    }
}

impl Drop for QConn {
    fn drop(&mut self) {
        self.q_conn.close(Default::default(), &[]);
    }
}
