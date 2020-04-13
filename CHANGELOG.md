# quic-p2p - Change Log

## [0.5.0]
- Update quinn to 0.6.0
- Update rustls to 0.17.0

## [0.4.0]
- Force the use of the basic single-threaded Tokio scheduler to prevent conflicts when used by a crate using Tokio `rt-threaded` feature
- Take two channels, one for client event and a second one for a node
- Use node or client channel for sending a message depending on the peer we are receiving the message from
- Remove the use of peer certificate, and therefore remove it from the handshake process
- Use shared QUIC `ClientConfig` instead of one per peer
- Update for Rust 1.41 (mem::replace -> mem::take)
- Use structopt to parse command line arguments
- Rename `proxies` to `bootstrap_nodes`
- Migrate to async/await syntax with new quinn v0.5
- Update CI to run all packages in the worspace
- Migrate CI/CD pipeline to GitHub Actions
- Use new new-style macro import
- Unsent user messages in the pending queues of an ongoing connection attempt will now be sent back to the user library if the connection attempt fails.
- Report connection failure for all cases where the connection was initiated by us. Previously some of the cases where not handled.
- Fire unsent user messages to the clients back to the user library. Previously unsent messages to clients were silently ignored.

## [0.3.0]
- Expose `Dirs` and `OverRide` structs publicly.
- Add `boostrap_cache_dir` field to the config to specify a custom path for the bootstrap cache.

## [0.2.1]
- Fix incorrect deserialisation logic in `WireMsg`
- Fix `fmt::Display` for `Event` and `WireMsg`

## [0.2.0]
- Fix bugs
- Modify API and internals with changes required by routing
- Return user messages given via `send` API for both successful and unsuccessful sends
- Tie a user given token to the event returning the above message to help identify the context

## [0.1.1]
- Initial release.
- Implement bootstrap cache.
- Implement the bootstrap logic.
- Add peer types (Client/Node).
- Implement optimised user message transfer (for larger messages).
- Add utils for testing delayed connections.
- Add configuration loading from files and command line (with `structopt`).
