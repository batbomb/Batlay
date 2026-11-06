import { describe, it, expect } from "vitest";
import { cleanMetadataText, repairMojibake } from "@/utils/text-encoding";

/** Encode en page de code OEM 850 (celle d'une console Windows française) les seuls caractères dont on a besoin. */
const CP850: Record<string, number> = { é: 0x82, è: 0x8a, à: 0x85, ç: 0x87, Ç: 0x80, ê: 0x88, ô: 0x93 };
function encodeCp850(text: string): Buffer {
  return Buffer.from([...text].map((ch) => (ch in CP850 ? CP850[ch] : ch.charCodeAt(0))));
}

describe("métadonnées mal décodées par la SOURCE (tags MP3 UTF-8 lus en Windows-1252)", () => {
  const cases: [string, string][] = [
    ["CafÃ©", "Café"],
    ["AngÃ¨le", "Angèle"],
    ["FranÃ§ois Hardy", "François Hardy"],
    ["Ã‰tienne Daho", "Étienne Daho"], // É = C3 89 ; 0x89 lu en Windows-1252 = ‰
    ["Beyoncé".normalize("NFC"), "Beyoncé"], // déjà correct : inchangé
    ["Sinéad O’Connor", "Sinéad O’Connor"], // apostrophe typographique légitime : inchangé
  ];
  it.each(cases)("%s -> %s", (input, expected) => {
    expect(cleanMetadataText(input)).toBe(expected);
  });

  it("répare un double encodage", () => {
    const once = Buffer.from("Café", "utf8").toString("latin1"); // CafÃ©
    const twice = Buffer.from(once, "utf8").toString("latin1");
    expect(repairMojibake(twice)).toBe("Café");
  });

  it("ne « répare » PAS un texte légitime qui contient un Ã ou un Â", () => {
    expect(repairMojibake("SÃO PAULO")).toBe("SÃO PAULO");
    expect(repairMojibake("Bâ Ã")).toBe("Bâ Ã");
  });

  it("ne masque pas un � déjà perdu (il doit rester visible pour signaler le problème en amont)", () => {
    expect(cleanMetadataText("Caf\ufffd")).toBe("Caf\ufffd");
  });

  it("retire les caractères de contrôle (NUL final des tags) et normalise en NFC", () => {
    expect(cleanMetadataText("Titre\u0000\u0000")).toBe("Titre");
    expect(cleanMetadataText("Cafe\u0301")).toBe("Café"); // e + accent combinant -> é
    expect(cleanMetadataText("  espaces  ")).toBe("espaces");
    expect(cleanMetadataText(null)).toBe("");
    expect(cleanMetadataText(undefined)).toBe("");
  });
});
