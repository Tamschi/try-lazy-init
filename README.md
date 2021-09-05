# try-lazy-init

[![Lib.rs](https://img.shields.io/badge/Lib.rs-*-84f)](https://lib.rs/crates/try-lazy-init)
[![Crates.io](https://img.shields.io/crates/v/try-lazy-init)](https://crates.io/crates/try-lazy-init)
[![Docs.rs](https://docs.rs/try-lazy-init/badge.svg)](https://docs.rs/try-lazy-init)

![Rust 1.51](https://img.shields.io/static/v1?logo=Rust&label=&message=1.51&color=grey)
[![CI](https://github.com/Tamschi/try-lazy-init/workflows/CI/badge.svg?branch=develop)](https://github.com/Tamschi/try-lazy-init/actions?query=workflow%3ACI+branch%3Adevelop)
![Crates.io - License](https://img.shields.io/crates/l/try-lazy-init/0.0.1)

[![GitHub](https://img.shields.io/static/v1?logo=GitHub&label=&message=%20&color=grey)](https://github.com/Tamschi/try-lazy-init)
[![open issues](https://img.shields.io/github/issues-raw/Tamschi/try-lazy-init)](https://github.com/Tamschi/try-lazy-init/issues)
[![open pull requests](https://img.shields.io/github/issues-pr-raw/Tamschi/try-lazy-init)](https://github.com/Tamschi/try-lazy-init/pulls)
[![crev reviews](https://web.crev.dev/rust-reviews/badge/crev_count/try-lazy-init.svg)](https://web.crev.dev/rust-reviews/crate/try-lazy-init/)

This is a straightforward fork of the [`lazy-init`](https://crates.io/crates/lazy-init), with fallible initialisation added that I couldn't provide with a wrapper.

## Installation

Please use [cargo-edit](https://crates.io/crates/cargo-edit) to always add the latest version of this library:

```cmd
cargo add try-lazy-init
```

## Example

```rust
use try_lazy_init::{Lazy, LazyTransform};

let lazy = Lazy::new();
assert_eq!(
  // Not yet initialized, so this closure runs:
  lazy.get_or_create(|| 1),
  &1
);
assert_eq!(
  // Already initialized so this closure doesn't run:
  lazy.try_get_or_create(|| { unreachable!(); Err(()) }),
  Ok(&1)
);
assert_eq!(lazy.get(), Some(&1));
assert_eq!(lazy.into_inner(), Some(1));

let lazy_transform = LazyTransform::new(1);
assert_eq!(
  // Not yet initialized, so this closure runs:
  lazy_transform.get_or_create(|x| x + 1),
  &2
);
assert_eq!(
  // Only available `where T: Clone`.
  // Already initialized so this closure doesn't run:
  lazy_transform.try_get_or_create(|_| { unreachable!(); Err(()) }),
  Ok(&2)
);
assert_eq!(lazy_transform.get(), Some(&2));
assert_eq!(lazy_transform.into_inner(), Ok(2));
```

## License

As of 2021-09-05 (i.e. the date of the fork), `lazy-init`'s *Cargo.toml* states `license = "Apache-2.0/MIT"` while only the MIT License's text is present in *LICENSE*. I've left this situation unchanged.

As such, consider the package MIT-licensed for use and Apache-2.0/MIT dual-licensed when contributing.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## [Code of Conduct](CODE_OF_CONDUCT.md)

## [Changelog](CHANGELOG.md)

## Versioning

`try-lazy-init` strictly follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) with the following exceptions:

* The minor version will not reset to 0 on major version changes (except for v1).  
Consider it the global feature level.
* The patch version will not reset to 0 on major or minor version changes (except for v0.1 and v1).  
Consider it the global patch level.

This includes the Rust version requirement specified above.  
Earlier Rust versions may be compatible, but this can change with minor or patch releases.

Which versions are affected by features and patches can be determined from the respective headings in [CHANGELOG.md](CHANGELOG.md).
