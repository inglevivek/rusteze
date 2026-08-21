# rusteze

rusteze is a small collection of Rust-based projects and supporting tooling, with complementary Python, HTML, and JavaScript components.

## Language composition
This repository currently contains code written in the following languages:

- Rust: 56.6%
- Python: 26.2%
- HTML: 15.3%
- JavaScript: 1.9%

## Overview
The primary focus of this repository is Rust development: libraries, CLI tools, and examples are organized under the repository root. Python is used for supporting scripts, tests, or tooling; HTML and JavaScript are included for any web or UI examples.

## Quick start
Prerequisites:

- Rust and Cargo (install from https://rustup.rs)
- Python 3.8+ (for supporting scripts)
- Node.js / npm (only if you plan to work on any web examples)

Build and run the Rust code:

```bash
# build in debug
cargo build

# build release
cargo build --release

# run tests
cargo test
```

Formatting and linting:

```bash
# format the code
cargo fmt --all

# run clippy for linting
cargo clippy --all-targets --all-features -- -D warnings
```

Python components (if present):

```bash
python -m venv .venv
source .venv/bin/activate  # or .venv\Scripts\activate on Windows
pip install -r requirements.txt
```

Web components (if present):

```bash
# from the web/ or frontend/ directory, if applicable
npm install
npm run build
```

## Contributing
Contributions are welcome. Please:

1. Open an issue to discuss larger changes.
2. Create a branch for your work: `git checkout -b feature/your-feature`.
3. Run tests and linters before submitting a pull request.

Be sure to follow Rust formatting and linting rules (`cargo fmt`, `cargo clippy`).

## License
See the LICENSE file in this repository for licensing details.

## Issues and support
If you find a bug or want to request a feature, please open an issue in this repository.
