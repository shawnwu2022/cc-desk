# Public TEST_ONLY certificate fixtures

These exact public fixtures come from the official crates.io tokio-rustls0.26.4
package, tests/certs/{root.pem,chain.pem,end.key}; upstream repository:
https://github.com/rustls/tokio-rustls/tree/v/0.26.4/tests/certs
The source crate is pinned/checksummed in remote/Cargo.lock. Upstream licenses are
preserved here. No certificate/key was generated, enrolled, or installed.

The universally known private test key is not an access credential. Never enroll
this CA/key anywhere outside a TEST_ONLY in-memory test. Only cfg(test) code imports
these files; production client uses the pinned Mozilla WebPKI root bundle and cannot
accept caller-injected roots. Fixtures certify foobar.com; tests resolve it explicitly
to owned loopback sockets, never DNS or the public domain.
