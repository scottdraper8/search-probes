FROM docker.io/library/fedora:44@sha256:15da97ed9aa94dd3d92c3925eb89520f6c80e07ea7ee24cba848d1f8b6912b3e

RUN dnf install --assumeyes --setopt=install_weak_deps=False \
        gcc \
        gcc-c++ \
        mingw64-binutils \
        mingw64-gcc \
        mingw64-gcc-c++ \
    && dnf clean all
