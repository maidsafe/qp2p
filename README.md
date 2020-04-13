# quic-p2p

|Crate|Documentation|Linux/macOS/Windows|
|:---:|:-----------:|:-----------------:|
|[![](http://meritbadge.herokuapp.com/quic-p2p)](https://crates.io/crates/quic-p2p)|[![Documentation](https://docs.rs/quic-p2p/badge.svg)](https://docs.rs/quic-p2p)|[![Build Status](https://travis-ci.com/maidsafe/quic-p2p.svg?branch=master)](https://travis-ci.com/maidsafe/quic-p2p)|

| [MaidSafe website](https://maidsafe.net) | [SAFE Dev Forum](https://forum.safedev.org) | [SAFE Network Forum](https://safenetforum.org) |
|:-------------------------------------:|:---------------------------------------:|:------------------------------------------:|

## Overview

This library provides a mechanism for peers on P2P networks to communicate
securely. It also allows the network peers to re-join the network without
requiring them to re-connect to any known peers. These peers such as hard coded
peers or DNS defined peers as these are obviously a security concern and
a centralised set of peers that can easily be attacked or even torn down. There
are several informative posts describing both QUIC and TLS 1.3:

- [The IETF draft specification](https://tools.ietf.org/html/draft-ietf-quic-transport-20#section-6)
- [Cloudflare intro to QUIC](https://blog.cloudflare.com/the-road-to-quic/)
- [Cloudflare intro to TLS 1.3](https://www.cloudflare.com/learning-resources/tls-1-3/)

These are highly recommended to be able to better understand this library, in
particular the Cloudflare blog posts (10 minute read).

### Encryption of connections

QUIC provides connection security via the use of TLS 1.3. This library allows 3 different connection types with regard to encryption and validation.

1. Require peers to have certificates from an agreed certificate authority.
1. Allow use of a private certificate authority.
1. Allow no identity validation of peers, but do encrypt connections.

This should satisfy the requirements of many P2P networks, whether they trust any clearnet certificate authority (which may be a centralised attack source) or whether they pass the identity management up to a different layer to validate identities and simply use quic-p2p as a secured network in terms of encrypted connections.

### Bootstrap Cache

quic-p2p will save any endpoints and certificates of nodes that are connectible without any setup such as is required by NAT hole punching. The most recently connected 200 nodes are stored and these are then used to re-join the network after any restart.

### Connectivity types

quic-p2p uses 2 connection types when in P2P mode. This allows the connections to be defined as:

1. A bi-directional connection.

1. A uni-directional connection.

Where 1 allows connections from consumers of the network, such as clients or
perhaps P2P nodes that are simply obtaining information, such as bootstrapping,
2 is used where the network is allowing another P2P worker. These peers must be
both able to connect and be able to be connected to. Using a uni-directional
stream per connection forces the node to confirm both incoming and outgoing
connectivity is available.

A peer my also use a bi-directional connection where it is using STUN or TURN
to make that connection. It has to however introduce itself as a Client and
not a Node as Nodes are always checked for reverse connectivity to them.

### IP Spoof defence

This library enables `stateless-retry` to defend against IP spoofing. This is
achieved by sending a token back to the connecting node which must be returned.
QUIC also defines a protocol negotiation process that defend against many
attacks by confirming the acceptable protocols. This is defined
[here](https://tools.ietf.org/html/draft-ietf-quic-transport-03#section-7.1).

## TODO

- [ ] Hole punching for NAT traversal
- [ ] Support for async/await syntax
- [ ] Benchmarks and more examples

## License

This SAFE Network library is dual-licensed under the Modified BSD ([LICENSE-BSD](LICENSE-BSD) https://opensource.org/licenses/BSD-3-Clause) or the MIT license ([LICENSE-MIT](LICENSE-MIT) http://opensource.org/licenses/MIT) at your option.

## Contributing

Want to contribute? Great :tada:

There are many ways to give back to the project, whether it be writing new code, fixing bugs, or just reporting errors. All forms of contributions are encouraged!

For instructions on how to contribute, see our [Guide to contributing](https://github.com/maidsafe/QA/blob/master/CONTRIBUTING.md).
