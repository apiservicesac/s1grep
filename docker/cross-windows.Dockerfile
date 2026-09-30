# Cross-compiles s1grep for x86_64-pc-windows-gnu; used by ./dev.sh windows.
FROM rust:1-trixie
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-pc-windows-gnu
