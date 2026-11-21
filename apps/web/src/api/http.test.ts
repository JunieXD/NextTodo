// Where the sign-in page sends an address typed without a scheme (docs/design.md#直接用-http).

import { expect, test } from "vitest";
import { serverUrl, setServer } from "./http";

test("an address on the local network means plain HTTP, any other HTTPS", () => {
  const nearby = ["192.168.1.10:8080", "10.0.0.2", "172.20.1.1:8080", "127.0.0.1:8080", "100.101.1.2", "169.254.1.1", "nas:8080", "localhost", "localhost.:8080", "nas.local", "nas.local.", "router.lan", "todo.home.arpa.:8080", "[fd00::1]:8080", "[::1]:8080", "[fe80::1]"];
  const far = ["todo.example.com", "203.0.113.10", "172.32.0.1", "100.128.0.1", "8.8.8.8:8443", "[2001:db8::1]", "[fd::1]", "local.example.com"];
  for (const address of nearby) {
    setServer(address);
    expect(serverUrl(), address).toBe(`http://${address}`);
  }
  for (const address of far) {
    setServer(address);
    expect(serverUrl(), address).toBe(`https://${address}`);
  }
  // As typed, when the scheme is.
  setServer("http://todo.example.com/");
  expect(serverUrl()).toBe("http://todo.example.com");
  setServer("https://192.168.1.10");
  expect(serverUrl()).toBe("https://192.168.1.10");
  setServer("HTTP://Todo.example.com");
  expect(serverUrl()).toBe("http://Todo.example.com");
});
