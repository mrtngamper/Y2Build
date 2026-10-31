# Pinned build environment for Y2Linux + Y2Reborn: the checks (cargo fmt/clippy/test, the platform
# gate) and the image build both run here, so the host needs only just and docker.
#
# Y2Linux itself builds on the host (its docs/BUILDING.md lists the host packages); this image
# holds the same tools, with every input fixed:
#   - the Debian base by digest,
#   - every apt package from one snapshot.debian.org timestamp,
#   - rustup-init by version and SHA-256 (the toolchain itself is fixed by its version; rustup
#     checks it against the release manifest's hashes),
#   - FFmpeg for Reborn's host build from the archive and SHA-256 Y2Linux pins in
#     tools/production/ffmpeg9.py (Debian 13 ships FFmpeg 7.1; Reborn needs Y2Reborn/FFMPEG_VERSION).
# The justfile tags the image with the hash of this file, so changing a pin builds a new image.
#
# Looking up new pins:
#   debian digest: skopeo inspect --format '{{.Digest}}' docker://docker.io/library/debian:13
#                  (or: docker buildx imagetools inspect debian:13)
#   snapshot:      a timestamp from https://snapshot.debian.org/archive/debian/
#   rustup-init:   https://static.rust-lang.org/rustup/archive/<version>/x86_64-unknown-linux-gnu/rustup-init.sha256
#   ffmpeg:        ARCHIVE_SHA256 in Y2Linux/tools/production/ffmpeg9.py
FROM docker.io/library/debian:13@sha256:2dc419c6d7fe3dad0a2f9fd469c34cc90b0f9ba642d8fde837fe3d7d34a0e7f1 AS base
ARG DEBIAN_SNAPSHOT=20261030T000000Z
ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8
# apt checks the snapshot's signatures with the base image's keyring; Check-Valid-Until is off
# because a snapshot's Release file expires by design.
RUN rm -f /etc/apt/sources.list /etc/apt/sources.list.d/* \
    && for archive in debian:"trixie trixie-updates" debian-security:trixie-security; do \
        printf 'Types: deb\nURIs: http://snapshot.debian.org/archive/%s/%s\nSuites: %s\nComponents: main\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\nCheck-Valid-Until: no\n\n' \
            "${archive%%:*}" "$DEBIAN_SNAPSHOT" "${archive#*:}"; \
    done > /etc/apt/sources.list.d/snapshot.sources \
    && printf 'Acquire::Retries "5";\n' > /etc/apt/apt.conf.d/80retries

# --- FFmpeg (shared libraries only, nothing autodetected) ------------------------------------
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

# --- the build environment ---------------------------------------------------------------------
FROM base
ARG UID=1000
ARG GID=1000
ARG RUST_VERSION=1.99.0
ARG RUSTUP_VERSION=1.29.2
ARG RUSTUP_SHA256=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4

RUN apt-get update && apt-get install -y --no-install-recommends \
        # Buildroot host packages (Y2Linux docs/BUILDING.md §1)
        gcc g++ make binutils patch perl rsync cpio unzip bzip2 gzip xz-utils tar file bc wget \
        debianutils diffutils findutils gawk git sed bash \
        # build.py: the sandbox for the kernel build, downloads
        bubblewrap ca-certificates curl \
        # Python >= 3.12 with the tools' third-party modules
        python3 python3-pyelftools python3-pil \
        # packaging and validation: mke2fs/debugfs/e2fsck/dumpe2fs, blkid, simg2img, ssh-keygen, zip
        e2fsprogs util-linux android-sdk-libsparse-utils openssh-client zip \
        # host tests (tools/development/host-tests.sh)
        clang qemu-user \
        # Reborn's host build (Y2Reborn readme, Development); FFmpeg comes from the stage above
        pkg-config zlib1g-dev libasound2-dev libdbus-1-dev libsqlite3-dev \
        libdrm-dev libgbm-dev libegl-dev libgles-dev libblkid-dev \
        # Reborn's Bluetooth/AVRCP tests start a private bus
        dbus-daemon \
        # Y2Flash (built here, run on the host): serialport finds the player through libudev
        libudev-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg /opt/ffmpeg /opt/ffmpeg
RUN echo /opt/ffmpeg/lib > /etc/ld.so.conf.d/ffmpeg.conf && ldconfig
ENV PKG_CONFIG_PATH=/opt/ffmpeg/lib/pkgconfig

# The build runs as an ordinary user with the host's IDs, so files in out/, .cache/ and target/
# stay yours.
RUN groupadd -g "$GID" builder && useradd -m -u "$UID" -g "$GID" -s /bin/bash builder
USER builder
ENV HOME=/home/builder \
    PATH=/home/builder/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# Buildroot's reborn and y2-platform recipes call $(HOME)/.cargo/bin/cargo.
RUN curl --proto '=https' --tlsv1.2 -sSfo /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
        --target armv7-unknown-linux-gnueabihf -c rustfmt -c clippy \
    && rm /tmp/rustup-init \
    && cargo --version

WORKDIR /home/builder
