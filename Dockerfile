FROM scratch
ARG BIN
COPY dist/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY ${BIN} /usr/local/bin/bunnynet_prometheus
EXPOSE 9000
ENTRYPOINT ["/usr/local/bin/bunnynet_prometheus"]
