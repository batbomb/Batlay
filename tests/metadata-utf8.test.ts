import { describe, it, expect, vi } from "vitest";
import { cleanMetadataText } from "@/utils/text-encoding";

/** Ce que le script PowerShell écrit sur stdout : JSON -> octets UTF-8 -> Base64 (ASCII pur). */
const emit = (obj: unknown): string => Buffer.from(JSON.stringify(obj), "utf8").toString("base64");
const asStdout = (text: string): Buffer => Buffer.from(text, "latin1");

describe("cleanMetadataText — nettoyage sans jamais réécrire un nom", () => {
  const PROTECTED = [
    "Tyler, The Creator",
    "Simon & Garfunkel",
    "AC/DC",
    "Earth, Wind & Fire",
    "Mötley Crüe",
    "!!!",
    "Tyler, The Creator, Frank Ocean & Kali Uchis",
    "Sigur Rós",
    "MØ",
    "宇多田ヒカル",
  ];
  it.each(PROTECTED)("« %s » ressort tel quel", (name) => {
    expect(cleanMetadataText(name)).toBe(name);
  });

  it("ne découpe pas une liste d'artistes : virgules et « & » conservés", () => {
    expect(cleanMetadataText("Artiste 1, Artiste 2, Artiste 3")).toBe("Artiste 1, Artiste 2, Artiste 3");
  });

  it("supprime BOM et caractères de largeur nulle", () => {
    expect(cleanMetadataText("\uFEFFTitre")).toBe("Titre");
    expect(cleanMetadataText("Ti\u200Btre")).toBe("Titre");
    expect(cleanMetadataText("Ti\u2060tre")).toBe("Titre");
    expect(cleanMetadataText("\u200B\u200B")).toBe("");
  });

  it("supprime les caractères de contrôle et les NUL de fin de tag", () => {
    expect(cleanMetadataText("Titre\u0000\u0000")).toBe("Titre");
    expect(cleanMetadataText("A\u0007B\u001fC\u007fD\u0085E")).toBe("ABCDE");
  });

  it("ramène espaces multiples, tabulations, retours à la ligne et insécables à un seul espace", () => {
    expect(cleanMetadataText("  Daft   Punk  ")).toBe("Daft Punk");
    expect(cleanMetadataText("Titre\t\tlong\r\nsuite")).toBe("Titre long suite");
    expect(cleanMetadataText("A\u00A0\u00A0B")).toBe("A B");
    expect(cleanMetadataText("A\u3000B")).toBe("A B");
  });

  it("ZWJ / ZWNJ à l'intérieur d'un texte sont conservés (emoji composés, persan) ; en bordure ils sont retirés", () => {
    const family = "👨\u200D👩\u200D👧";
    expect(cleanMetadataText(family)).toBe(family);
    const persian = "می\u200Cخواهم";
    expect(cleanMetadataText(persian)).toBe(persian);
    expect(cleanMetadataText("\u200DTitre\u200C")).toBe("Titre");
  });

  it("NFC : « é » décomposé -> « é » simple", () => {
    expect(cleanMetadataText("Cafe\u0301")).toBe("Café");
    expect(cleanMetadataText("Ange\u0300le")).toBe("Angèle");
  });

  it("vide / null / undefined -> chaîne vide", () => {
    expect(cleanMetadataText("")).toBe("");
    expect(cleanMetadataText(null)).toBe("");
    expect(cleanMetadataText(undefined)).toBe("");
  });

  it("idempotente : nettoyer deux fois = nettoyer une fois", () => {
    for (const value of [...PROTECTED, "  \uFEFFAngè\u200Ble \t 🎵\u0000 ", "CafÃ©", "SÃO PAULO"]) {
      const once = cleanMetadataText(value);
      expect(cleanMetadataText(once)).toBe(once);
    }
  });
});
