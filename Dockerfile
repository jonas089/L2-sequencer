# Use an official Rust image as a base image
FROM rust:1.81.0

# Set the working directory inside the container
WORKDIR /usr/src/app

# Install the necessary tools for building the risc0 toolchain
RUN apt-get update && apt-get install -y curl

# Install cargo-binstall (used to install cargo-risczero)
RUN cargo install cargo-binstall

# Install the risc0 toolchain using cargo-risczero
RUN cargo binstall cargo-risczero -y
RUN cargo risczero install

# Copy the entire Rust project into the container
COPY . .

# Build the Rust project with the necessary feature
RUN cargo build --release --features sqlite

# Set the entrypoint to run the compiled binary
CMD ["./target/release/pord-sequencer"]