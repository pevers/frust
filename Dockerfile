FROM ubuntu:18.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -yq \
  curl \
  build-essential \
  && rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
WORKDIR /build
VOLUME [ "/build" ]
CMD ["/bin/bash"]