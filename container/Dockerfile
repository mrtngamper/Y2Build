# Pinned build environment for Y2Linux and Y2Reborn; every input is fixed by digest, snapshot or SHA-256.
FROM docker.io/library/debian:13@sha256:2dc419c6d7fe3dad0a2f9fd469c34cc90b0f9ba642d8fde837fe3d7d34a0e7f1 AS base
ARG DEBIAN_SNAPSHOT=20261030T000000Z
ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8
# All apt packages come from one snapshot.debian.org timestamp.
RUN rm -f /etc/apt/sources.list /etc/apt/sources.list.d/* \
    && for archive in debian:"trixie trixie-updates" debian-security:trixie-security; do \
        printf 'Types: deb\nURIs: http://snapshot.debian.org/archive/%s/%s\nSuites: %s\nComponents: main\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\nCheck-Valid-Until: no\n\n' \
            "${archive%%:*}" "$DEBIAN_SNAPSHOT" "${archive#*:}"; \
    done > /etc/apt/sources.list.d/snapshot.sources \
    && printf 'Acquire::Retries "5";\n' > /etc/apt/apt.conf.d/80retries

# FFmpeg for Reborn's host build (the version Y2Linux pins in tools/production/ffmpeg9.py).
FROM base AS ffmpeg
ARG FFMPEG_VERSION=9.0.2
ARG FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl xz-utils gcc make nasm pkg-config zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSfo /tmp/ffmpeg.tar.xz \
        "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" \
    && echo "${FFMPEG_SHA256}  /tmp/ffmpeg.tar.xz" | sha256sum -c - \
    && mkdir /tmp/ffmpeg && tar -xJf /tmp/ffmpeg.tar.xz -C /tmp/ffmpeg --strip-components=1 \
    && cd /tmp/ffmpeg \
    && ./configure --prefix=/opt/ffmpeg --enable-shared --disable-static --disable-programs \
        --disable-doc --disable-network --disable-autodetect --enable-zlib \
    && make -j"$(nproc)" && make install

# SQLite 3.51 and zlib-ng 2.1.5 match the machine the platform gate's goldens were recorded on.
FROM base AS goldens
ARG SQLITE_VERSION=3510200
ARG SQLITE_YEAR=2026
ARG SQLITE_SHA256=fbd89f866b1403bb66a143065440089dd76100f2238314d92274a082d4f2b7bb
ARG ZLIB_NG_VERSION=2.1.5
ARG ZLIB_NG_SHA256=3f6576971397b379d4205ae5451ff5a68edf6c103b2f03c4188ed7075fbb5f04
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev make \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSfLo /tmp/sqlite.tar.gz \
        "https://www.sqlite.org/${SQLITE_YEAR}/sqlite-autoconf-${SQLITE_VERSION}.tar.gz" \
    && echo "${SQLITE_SHA256}  /tmp/sqlite.tar.gz" | sha256sum -c - \
    && mkdir /tmp/sqlite && tar -xzf /tmp/sqlite.tar.gz -C /tmp/sqlite --strip-components=1 \
    && cd /tmp/sqlite && ./configure --prefix=/opt/sqlite --disable-static \
    && make -j"$(nproc)" && make install
RUN curl --proto '=https' --tlsv1.2 -sSfLo /tmp/zlib-ng.tar.gz \
        "https://github.com/zlib-ng/zlib-ng/archive/refs/tags/${ZLIB_NG_VERSION}.tar.gz" \
    && echo "${ZLIB_NG_SHA256}  /tmp/zlib-ng.tar.gz" | sha256sum -c - \
    && mkdir /tmp/zlib-ng && tar -xzf /tmp/zlib-ng.tar.gz -C /tmp/zlib-ng --strip-components=1 \
    && cd /tmp/zlib-ng && ./configure --zlib-compat --prefix=/opt/zlib-ng \
    && make -j"$(nproc)" && make install

# The build environment.
FROM base
ARG UID=1000
ARG GID=1000
ARG RUST_VERSION=1.99.0
ARG RUSTUP_VERSION=1.29.2
ARG RUSTUP_SHA256=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4

RUN apt-get update && apt-get install -y --no-install-recommends \
        # Buildroot host packages (Y2Linux docs/BUILDING.md)
        gcc g++ make binutils patch perl rsync cpio unzip bzip2 gzip xz-utils tar file bc wget \
        debianutils diffutils findutils gawk git sed bash \
        # build.py: kernel build sandbox and downloads
        bubblewrap ca-certificates curl \
        # Python and the tools' modules
        python3 python3-pyelftools python3-pil \
        # packaging and validation
        e2fsprogs util-linux android-sdk-libsparse-utils openssh-client zip \
        # host tests
        clang qemu-user \
        # Reborn's host build
        pkg-config zlib1g-dev libasound2-dev libdbus-1-dev libsqlite3-dev \
        libdrm-dev libgbm-dev libegl-dev libgles-dev libblkid-dev \
        # Reborn's Bluetooth tests
        dbus-daemon \
        # Y2Flash
        libudev-dev \
        # Rust compile cache (every cargo build in the container: just _in, and the build units)
        sccache \
        # JSON in the build units (versions.json, the keys)
        jq \
        # Ed25519 for the update packages of `just push system` (Y2Linux tools/update/package.py)
        libsodium23 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg /opt/ffmpeg /opt/ffmpeg
RUN echo /opt/ffmpeg/lib > /etc/ld.so.conf.d/ffmpeg.conf && ldconfig
ENV PKG_CONFIG_PATH=/opt/ffmpeg/lib/pkgconfig

# SQLite 3.51 replaces Debian's; zlib-ng is only used by `just gate`; goldens expect Berlin time.
COPY --from=goldens /opt/sqlite/lib /opt/sqlite/lib
COPY --from=goldens /opt/zlib-ng/lib /opt/zlib-ng/lib
RUN echo /opt/sqlite/lib > /etc/ld.so.conf.d/00-sqlite.conf && ldconfig \
    && ln -sf /usr/share/zoneinfo/Europe/Berlin /etc/localtime && echo Europe/Berlin > /etc/timezone

# Build as a user with the host's IDs, so the output files stay yours.
RUN groupadd -g "$GID" builder && useradd -m -u "$UID" -g "$GID" -s /bin/bash builder
USER builder
ENV HOME=/home/builder \
    PATH=/home/builder/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# Rust, where Buildroot's recipes expect it ($HOME/.cargo/bin/cargo).
RUN curl --proto '=https' --tlsv1.2 -sSfo /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
        --target armv7-unknown-linux-gnueabihf -c rustfmt -c clippy \
    && rm /tmp/rustup-init \
    && cargo --version

WORKDIR /home/builder
