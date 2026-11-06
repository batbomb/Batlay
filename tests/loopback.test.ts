import { describe, it, expect } from "vitest";
import { HOSTS_FILE_LINE, LOCAL_ALIAS_HOSTNAME, isLoopbackHttpUrl } from "../shared/loopback";

describe("URL locales", () => {
  it("http + localhost/127.0.0.1 + port explicite, sans identifiants", () => {
    expect(isLoopbackHttpUrl("http://localhost:3000/overlay/a")).toBe(true);
    expect(isLoopbackHttpUrl("http://127.0.0.1:4000/overlay/chat?size=30")).toBe(true);
    expect(isLoopbackHttpUrl("https://localhost:3000/overlay/a")).toBe(false);
    expect(isLoopbackHttpUrl("http://localhost/overlay/a")).toBe(false);
    expect(isLoopbackHttpUrl("http://a:b@localhost:3000/")).toBe(false);
    expect(isLoopbackHttpUrl("http://10.0.0.2:3000/")).toBe(false);
    expect(isLoopbackHttpUrl("::")).toBe(false);
  });
});
