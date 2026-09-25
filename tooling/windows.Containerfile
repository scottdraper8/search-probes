FROM docker.io/library/fedora:44
RUN dnf install -y gcc gcc-c++ mingw64-gcc mingw64-gcc-c++ && dnf clean all
