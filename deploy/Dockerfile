# The server and the built web app, nothing else. Built by deploy.sh from a staging directory that
# holds the static Linux binary and `web/`.
FROM scratch
COPY nexttodo-server /nexttodo-server
COPY web /web
ENV NEXTTODO_DATA=/data NEXTTODO_WEB=/web NEXTTODO_LISTEN=0.0.0.0:8080
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/nexttodo-server"]
CMD ["serve"]
