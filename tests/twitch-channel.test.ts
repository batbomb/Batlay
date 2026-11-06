import { describe, it, expect } from "vitest";
import { parseTwitchChannel, twitchError } from "../shared/twitch";

describe("parseTwitchChannel : pseudo ou lien", () => {
  it("accepte un pseudo, avec @ ou #, en majuscules ou avec espaces", () => {
    expect(parseTwitchChannel("ninja")).toBe("ninja");
    expect(parseTwitchChannel("  @Ninja ")).toBe("ninja");
    expect(parseTwitchChannel("#Some_User_9")).toBe("some_user_9");
  });
  it("accepte les liens de chaîne, avec ou sans schéma, www, m., sous-page", () => {
    for (const url of [
      "https://www.twitch.tv/ninja",
      "http://twitch.tv/Ninja/",
      "twitch.tv/ninja",
      "www.twitch.tv/ninja?sr=a",
      "https://m.twitch.tv/ninja/videos",
      "https://www.twitch.tv/ninja#top",
    ]) {
      expect(parseTwitchChannel(url), url).toBe("ninja");
    }
  });
  it("accepte les liens de chat pop-out / modération / intégré", () => {
    expect(parseTwitchChannel("https://www.twitch.tv/popout/ninja/chat")).toBe("ninja");
    expect(parseTwitchChannel("https://www.twitch.tv/moderator/ninja")).toBe("ninja");
    expect(parseTwitchChannel("https://www.twitch.tv/embed/ninja/chat?parent=x.com")).toBe("ninja");
  });
  it("refuse tout le reste", () => {
    for (const bad of [
      "",
      "   ",
      "deux mots",
      "pseudo/avec/slash",
      "a".repeat(26),
      "nom-avec-tiret",
      "https://www.twitch.tv/",
      "https://www.twitch.tv/directory",
      "https://www.twitch.tv/videos/123456",
      "https://autre-site.com/ninja",
      "https://twitch.tv.evil.com/ninja",
      "https://nottwitch.tv/ninja",
      "<script>",
    ]) {
      expect(parseTwitchChannel(bad), bad).toBeNull();
    }
  });
});
