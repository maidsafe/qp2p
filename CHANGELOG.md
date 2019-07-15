# quic-p2p - Change Log

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
