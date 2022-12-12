# qp2p

|Crate|Documentation|
|:---:|:-----------:|
|[![](http://meritbadge.herokuapp.com/qp2p)](https://crates.io/crates/qp2p)|[![Documentation](https://docs.rs/qp2p/badge.svg)](https://docs.rs/qp2p)|

| [MaidSafe website](https://maidsafe.net) | [SAFE Dev Forum](https://forum.safedev.org) | [SAFE Network Forum](https://safenetforum.org) |
|:-------------------------------------:|:---------------------------------------:|:------------------------------------------:|

## Overview

This library provides an API to simplify common tasks when creating P2P networks, including:

- Establishing encrypted, uni- and bidirectional connections (using QUIC with TLS 1.3).
- Connection pooling, for reusing already opened connections.
- Fault-tolerance, including retries and racing connections against sets of peers.
- Sending discrete messages (using QUIC streams as 'framing').

## Features

### QUIC

There are several informative posts describing both QUIC and TLS 1.3:

- [The IETF draft specification](https://tools.ietf.org/html/draft-ietf-quic-transport-206)
- [Cloudflare intro to QUIC](https://blog.cloudflare.com/the-road-to-quic/)
- [Cloudflare intro to TLS 1.3](https://www.cloudflare.com/learning-resources/tls-1-3/)

These are highly recommended to be able to better understand this library, in particular the Cloudflare blog posts (10 minute read).

### Encryption of connections

QUIC provides connection security via the use of TLS 1.3.
Currently, self-signed certificates are used to encrypt connections, but this provides *no* authentication.

In the future, we would like to support authentication via certificate authorities and pre-agreed certificates.
This should satisfy the requirements of many P2P networks, whether they trust any clearnet certificate authority (which may be a centralised attack source) or whether they pass the identity management up to a different layer to validate identities and simply use qp2p as a secured network in terms of encrypted connections.

### Connection pooling

Although QUIC connections are relatively cheap to establish (e.g. compared to TCP connections), there is still some overhead.
To minimise this overhead, all opened connections are pooled and reused based on their target socket address.

### Fault tolerance

APIs are available to connect to any peer from a list of peers.
Connections are established concurrently, and the first to succeed is kept, while the rest are discarded.
This allows connecting to any of a set of equivalent peers, finding a still-reachable peer in a set of previously known peers, etc.

### Messaging

QUIC is a streaming protocol, without an explicit model for discrete messages.
However, QUIC connections can multiplex an arbitrary number of streams, which are cheap to construct and dispose of.
This library uses streams as a message framing mechanism, sending or receiving a single message per stream.

QUIC supports both unidirectional and bidirectional streams, and both are exposed through this library.

- Unidirectional streams should typically be preferred for peer-to-peer communication.
  This requires peers to have external connectivity, which is generally better for fault tolerance (e.g. contact sharing, reconnection).

- Bidirectional streams should be preferred for client-server communication.
  This does not require external connectivity, so clients can still communicate from behind firewalls/gateways (such as household routers).

## License

This SAFE Network library is dual-licensed under the Modified BSD ([LICENSE-BSD](LICENSE-BSD) https://opensource.org/licenses/BSD-3-Clause) or the MIT license ([LICENSE-MIT](LICENSE-MIT) http://opensource.org/licenses/MIT) at your option.

## Contributing

Want to contribute? Great 🎉

There are many ways to give back to the project, whether it be writing new code, fixing bugs, or just reporting errors. All forms of contributions are encouraged!

For instructions on how to contribute, see our [Guide to contributing](https://github.com/maidsafe/QA/blob/master/CONTRIBUTING.md).
