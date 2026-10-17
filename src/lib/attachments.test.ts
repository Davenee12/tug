import { describe, expect, it } from "vitest";
import { isUnreadable, mediaEmoji, mediaLabel, parseAttachments, previewLine, toastBody } from "./attachments";

describe("parseAttachments", () => {
  it("reads iOS's photo line as one photo", () => {
    expect(parseAttachments("Attachment: 1 Photo")).toEqual({ text: "", media: { kind: "photo", count: 1 } });
  });

  it("handles plurals, case and spacing", () => {
    expect(parseAttachments("Attachment: 2 Photos").media).toEqual({ kind: "photo", count: 2 });
    expect(parseAttachments("attachment: 3 photos").media).toEqual({ kind: "photo", count: 3 });
    expect(parseAttachments("ATTACHMENTS:2   VIDEOS").media).toEqual({ kind: "video", count: 2 });
    expect(parseAttachments("  Attachment: 1 Sticker  ").media).toEqual({ kind: "sticker", count: 1 });
  });

  it("knows the common kinds", () => {
    const kind = (word: string) => parseAttachments(`Attachment: 1 ${word}`).media?.kind;
    expect(kind("Video")).toBe("video");
    expect(kind("Audio")).toBe("audio");
    expect(kind("Audio Message")).toBe("audio");
    expect(kind("Image")).toBe("photo");
    expect(kind("GIF")).toBe("gif");
    expect(kind("File")).toBe("file");
    expect(kind("Contact")).toBe("contact");
    expect(kind("Location")).toBe("location");
  });

  it("keeps an unknown one-word kind as a plain attachment", () => {
    expect(parseAttachments("Attachment: 1 Hologram").media).toEqual({ kind: "attachment", count: 1 });
  });

  it("keeps the caption when the photo line has its own line", () => {
    expect(parseAttachments("Look at this!\nAttachment: 1 Photo")).toEqual({
      text: "Look at this!",
      media: { kind: "photo", count: 1 },
    });
    expect(parseAttachments("Attachment: 1 Photo\r\nfrom the beach\nso nice")).toEqual({
      text: "from the beach\nso nice",
      media: { kind: "photo", count: 1 },
    });
  });

  it("finds the photo phrase sharing a line with a caption", () => {
    expect(parseAttachments("Look at this Attachment: 1 Photo")).toEqual({ text: "Look at this", media: { kind: "photo", count: 1 } });
    expect(parseAttachments("Attachment: 2 Photos from today")).toEqual({ text: "from today", media: { kind: "photo", count: 2 } });
    expect(parseAttachments("Attachment: 1 Audio message listen")).toEqual({ text: "listen", media: { kind: "audio", count: 1 } });
  });

  it("adds up several lines; mixed kinds become attachments", () => {
    expect(parseAttachments("Attachment: 1 Photo\nAttachment: 2 Photos").media).toEqual({ kind: "photo", count: 3 });
    expect(parseAttachments("Attachment: 1 Photo\nAttachment: 1 Video").media).toEqual({ kind: "attachment", count: 2 });
  });

  it("treats U+FFFC (where an iMessage attachment sat) as an attachment of unknown kind", () => {
    expect(parseAttachments("￼")).toEqual({ text: "", media: { kind: "attachment", count: null } });
    expect(parseAttachments("so cute ￼")).toEqual({ text: "so cute", media: { kind: "attachment", count: null } });
  });

  it("leaves everything else exactly as it came", () => {
    for (const body of [
      "see you at 5",
      "",
      "Attachment",
      "Attachment: Photo",
      "Attachment: 2 are below",
      "The attachment: 1 photo is coming",
      "My attachment: 1 dog and 2 cats",
      "Anhang: 1 Foto",
      "Pièce jointe : 1 photo",
      "Attachment: 1000 Photos",
    ]) {
      expect(parseAttachments(body)).toEqual({ text: body, media: null });
    }
  });
});

describe("mediaLabel", () => {
  it("names one, counts several", () => {
    expect(mediaLabel({ kind: "photo", count: 1 })).toBe("Photo");
    expect(mediaLabel({ kind: "photo", count: 2 })).toBe("2 photos");
    expect(mediaLabel({ kind: "sticker", count: 1 })).toBe("Sticker");
    expect(mediaLabel({ kind: "video", count: 3 })).toBe("3 videos");
    expect(mediaLabel({ kind: "gif", count: 2 })).toBe("2 GIFs");
    expect(mediaLabel({ kind: "audio", count: 1 })).toBe("Audio message");
    expect(mediaLabel({ kind: "attachment", count: 2 })).toBe("2 attachments");
    expect(mediaLabel({ kind: "photo", count: 0 })).toBe("Photo");
  });

  it("says photo or attachment when the phone didn't say which", () => {
    expect(mediaLabel({ kind: "attachment", count: null })).toBe("Photo or attachment");
  });
});

describe("previews and pop-ups", () => {
  it("previews a photo as 📷 Photo, or 📷 and its caption", () => {
    expect(previewLine(parseAttachments("Attachment: 1 Photo"))).toBe("📷 Photo");
    expect(previewLine(parseAttachments("Attachment: 2 Photos"))).toBe("📷 2 photos");
    expect(previewLine(parseAttachments("Look!\nAttachment: 1 Photo"))).toBe("📷 Look!");
    expect(previewLine(parseAttachments("hello"))).toBe("hello");
    expect(mediaEmoji("video")).toBe("🎥");
  });

  it("puts the photo and where to see it in a pop-up", () => {
    expect(toastBody("Attachment: 1 Photo")).toBe("📷 Photo\nOpen it on your iPhone");
    expect(toastBody("Look!\nAttachment: 1 Photo")).toBe("Look!\n📷 Photo · Open it on your iPhone");
    expect(toastBody("hello")).toBe("hello");
    expect(toastBody("�")).toBe("This text didn't come through. Open it on your iPhone.");
  });
});

describe("isUnreadable", () => {
  it("is a body of only replacement characters", () => {
    expect(isUnreadable("�")).toBe(true);
    expect(isUnreadable(" �� ")).toBe(true);
    expect(isUnreadable("caf�")).toBe(false);
    expect(isUnreadable("")).toBe(false);
    expect(isUnreadable("ok")).toBe(false);
  });
});
