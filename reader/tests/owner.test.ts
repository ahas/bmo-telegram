import { describe, expect, test } from "bun:test";

import { albumSendOptions, attachmentKind, attachmentName, attachmentSendOptions } from "../src/owner";

describe("what Telegram makes of an attached file", () => {
  test("a JPEG and a PNG are photos", () => {
    expect(attachmentKind("image/jpeg", "IMG_0421.jpg")).toBe("photo");
    expect(attachmentKind("image/png", "shot.png")).toBe("photo");
    expect(attachmentKind("IMAGE/JPEG", "shot.JPG")).toBe("photo");
  });

  test("a GIF moves", () => {
    expect(attachmentKind("image/gif", "cat.gif")).toBe("animation");
  });

  test("any other image is kept whole as a file rather than recompressed into something else", () => {
    expect(attachmentKind("image/webp", "sticker.webp")).toBe("document");
    expect(attachmentKind("image/svg+xml", "chart.svg")).toBe("document");
    expect(attachmentKind("image/heic", "IMG_0001.heic")).toBe("document");
  });

  test("a video is a video and a sound is an audio", () => {
    expect(attachmentKind("video/mp4", "clip.mp4")).toBe("video");
    expect(attachmentKind("video/quicktime", "clip.mov")).toBe("video");
    expect(attachmentKind("audio/mpeg", "song.mp3")).toBe("audio");
    expect(attachmentKind("audio/ogg", "voice.ogg")).toBe("audio");
  });

  test("everything else is a file", () => {
    expect(attachmentKind("application/pdf", "report.pdf")).toBe("document");
    expect(attachmentKind("text/plain", "notes.txt")).toBe("document");
    expect(attachmentKind("application/octet-stream", "blob.bin")).toBe("document");
  });

  test("a file that arrived with no type of its own is read off its name", () => {
    expect(attachmentKind("", "IMG_0421.JPG")).toBe("photo");
    expect(attachmentKind("", "clip.mp4")).toBe("video");
    expect(attachmentKind("", "archive.tar.gz")).toBe("document");
    expect(attachmentKind("", "")).toBe("document");
  });
});

describe("the name a file travels under", () => {
  test("a photo pasted with no ending is given the one its type says", () => {
    expect(attachmentName("photo", "clipboard", "image/png")).toBe("clipboard.png");
    expect(attachmentName("photo", "clipboard", "image/jpeg")).toBe("clipboard.jpg");
    expect(attachmentName("photo", "", "image/jpeg")).toBe("attachment.jpg");
  });

  test("a photo that already says what it is keeps its own name", () => {
    expect(attachmentName("photo", "IMG_0421.jpg", "image/jpeg")).toBe("IMG_0421.jpg");
    expect(attachmentName("photo", "shot.PNG", "image/png")).toBe("shot.PNG");
  });

  test("every other kind keeps its name exactly", () => {
    expect(attachmentName("document", "final report.pdf", "application/pdf")).toBe("final report.pdf");
    expect(attachmentName("video", "clip.mkv", "video/x-matroska")).toBe("clip.mkv");
  });
});

describe("what the account is told", () => {
  test("only a document is forced to stay one, and only a video may stream", () => {
    expect(attachmentSendOptions("photo")).toEqual({ forceDocument: false, supportsStreaming: false, animated: false });
    expect(attachmentSendOptions("video")).toEqual({ forceDocument: false, supportsStreaming: true, animated: false });
    expect(attachmentSendOptions("animation")).toEqual({ forceDocument: false, supportsStreaming: false, animated: true });
    expect(attachmentSendOptions("audio")).toEqual({ forceDocument: false, supportsStreaming: false, animated: false });
    expect(attachmentSendOptions("document")).toEqual({ forceDocument: true, supportsStreaming: false, animated: false });
  });
});

describe("the albums a send of several goes as", () => {
  test("an album of documents stays documents, and one carrying a video streams", () => {
    expect(albumSendOptions(["document", "document"])).toEqual({ forceDocument: true, supportsStreaming: false, animated: false });
    expect(albumSendOptions(["photo", "video"])).toEqual({ forceDocument: false, supportsStreaming: true, animated: false });
  });
});
