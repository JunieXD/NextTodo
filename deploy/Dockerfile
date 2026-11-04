# The server and the built web app, nothing else. Built from the directory deploy/build.sh fills,
# which holds a static Linux server binary for each architecture and `web/`.
FROM scratch
ARG TARGETARCH
LABEL org.opencontainers.image.source=https://github.com/JunieXD/NextTodo \
      org.opencontainers.image.description="NextTodo 服务端和网页端" \
      org.opencontainers.image.licenses=MIT
COPY nexttodo-server-${TARGETARCH} /nexttodo-server
COPY web /web
ENV NEXTTODO_DATA=/data NEXTTODO_WEB=/web NEXTTODO_LISTEN=0.0.0.0:8080
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/nexttodo-server"]
CMD ["serve"]
