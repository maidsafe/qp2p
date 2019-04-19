// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::connect;
use crate::connection::BootstrapGroupBuilder;
use crate::context::ctx;

pub fn initiate() {
    let (proxies, event_tx): (Vec<_>, _) = ctx(|c| {
        (
            c.bootstrap_cache
                .peers()
                .iter()
                .rev()
                .chain(c.bootstrap_cache.hard_coded_contacts().iter())
                .cloned()
                .collect(),
            c.event_tx.clone(),
        )
    });

    let bootstrap_group_builder = BootstrapGroupBuilder::new(event_tx);
    for proxy in proxies {
        let _ = connect::connect_to(proxy, None, Some(&bootstrap_group_builder));
    }
}
