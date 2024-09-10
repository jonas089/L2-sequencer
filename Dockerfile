# Use the Rust official image
FROM rust:1.81.0

# Set the working directory
WORKDIR /usr/src/app

# Install necessary dependencies
RUN apt-get update && apt-get install -y curl cmake ninja-build python3

# Install cargo-binstall
RUN cargo install cargo-binstall

# Install cargo-risczero using binstall
RUN cargo binstall cargo-risczero -y

# Build the risc0 toolchain
RUN cargo risczero build-toolchain

# Copy the entire Rust project into the container
COPY . .

RUN rustup toolchain install nightly

RUN rustup default nightly

# Build the Rust project with the necessary feature
RUN cargo build --release --features sqlite

# Set the entrypoint to run the compiled binary
CMD ["./target/release/pord-sequencer"]