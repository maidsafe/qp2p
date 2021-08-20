// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::ConnectionError;
use std::sync::{Arc, Weak};
use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};
use tokio::sync::{broadcast, Mutex};

type Result = std::result::Result<(), ConnectionError>;

// Deduplicate multiple concurrent connect attempts to the same peer - they all will yield the
// same `Connection` instead of opening a separate connection each.
#[derive(Clone)]
pub(crate) struct ConnectionDeduplicator {
    map: Arc<Mutex<HashMap<SocketAddr, Weak<broadcast::Sender<Result>>>>>,
}

pub(crate) enum DedupHandle {
    New(Completion),
    Dup(Result),
}

pub(crate) struct Completion(Arc<broadcast::Sender<Result>>);

impl Completion {
    pub(crate) fn complete(self, result: Result) {
        let _ = self.0.send(result);
    }
}

impl ConnectionDeduplicator {
    pub(crate) fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Query if there already is a connect attempt to the given address.
    //
    // If this is the first connect attempt, it returns `DedupHandle::New`, and we should proceed
    // with establishing the connection and then call `send` on the wrapped sender. For any
    // subsequent connect attempt this returns `DedupHandle::Dup` with the result of that attempt.
    pub(crate) async fn query(&self, addr: &SocketAddr) -> DedupHandle {
        loop {
            let mut rx = {
                let mut map = self.map.lock().await;

                // clear dropped handles
                map.retain(|_, tx| tx.strong_count() > 0);

                match map.entry(*addr) {
                    Entry::Occupied(entry) => {
                        if let Some(tx) = entry.get().upgrade() {
                            tx.subscribe()
                        } else {
                            // attempt was dropped, try again
                            continue;
                        }
                    }
                    Entry::Vacant(entry) => {
                        let (tx, _) = broadcast::channel(1);
                        let tx = Arc::new(tx);
                        let _ = entry.insert(Arc::downgrade(&tx));
                        return DedupHandle::New(Completion(tx));
                    }
                }
            };

            if let Ok(result) = rx.recv().await {
                return DedupHandle::Dup(result);
            } else {
                // attempt was dropped, try again
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionDeduplicator, DedupHandle};
    use anyhow::{anyhow, Result};
    use futures::{
        future::{select_all, try_join_all},
        Future, TryFutureExt,
    };
    use std::{
        net::{Ipv4Addr, SocketAddr},
        time::Duration,
    };

    #[tokio::test]
    async fn many_concurrent_queries() -> Result<()> {
        let dedup = ConnectionDeduplicator::new();
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 1234));

        let mut queries: Vec<_> = (0..5)
            .map(|_| {
                let dedup = dedup.clone();
                Box::pin(async move { dedup.query(&addr).await })
            })
            .collect();

        // The first query should succeed
        let completion = if let Ok(DedupHandle::New(completion)) = timeout(&mut queries[0]).await {
            completion
        } else {
            return Err(anyhow!("Unexpected dup"));
        };

        // The remaining queries should block – use a short timeout to test
        let (res, _, _) = select_all((&mut queries[1..]).iter_mut().map(timeout)).await;
        assert!(res.is_err());

        // Now we complete the query
        let _ = completion.complete(Ok(()));

        // And everything should finish
        let rest = try_join_all((&mut queries[1..]).iter_mut().map(timeout)).await?;
        for handle in rest {
            if let DedupHandle::Dup(Ok(())) = handle {
                // ok
            } else {
                return Err(anyhow!("Unexpected new"));
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn cancellation() -> Result<()> {
        let dedup = ConnectionDeduplicator::new();
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 1234));

        // Two attempts – start a query, do some 'work', complete
        async fn work(dedup: ConnectionDeduplicator, addr: SocketAddr) -> Result<()> {
            match dedup.query(&addr).await {
                DedupHandle::Dup(res) => Ok(res?),
                DedupHandle::New(completion) => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    let _ = completion.complete(Ok(()));
                    Ok(())
                }
            }
        }
        let q1 = tokio::spawn(work(dedup.clone(), addr));
        let q2 = tokio::spawn(work(dedup.clone(), addr));

        // Cancel the first attempt after a short time
        tokio::time::sleep(Duration::from_millis(10)).await;
        q1.abort();

        // The 2nd attempt should still finish
        timeout(q2).await???;

        Ok(())
    }

    fn timeout<Fut: Future + Unpin>(fut: Fut) -> impl Future<Output = Result<Fut::Output>> + Unpin {
        Box::pin(tokio::time::timeout(Duration::from_millis(100), fut).err_into())
    }
}
