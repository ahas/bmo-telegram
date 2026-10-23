// The reader's routes, called by the service alone (docs/boundary-api.md, 「The reader's routes, called by the
// service」): whether the owner's account is connected, a picture or a file fetched through it, and a send in
// the owner's voice. Every route takes the seam's token.

import { fetchFile, fetchPicture } from "./media";
import { ownerRefusalName } from "./owner";

import type { TelegramClient } from "telegram";
import type { Peers } from "./media";
import type { AvatarKind } from "./store";
import type { OwnerAccount } from "./owner";

// One kept profile picture as the service lists it: whose it is, and when it was taken.
export interface KeptAvatar {
  kind: AvatarKind;
  id: number;
  takenAt: number;
}

export interface RoutesDeps {
  token: string;
  peers: Peers;
  owner: OwnerAccount;
  client: () => TelegramClient | null;
  // Every profile picture the reader keeps.
  avatars: () => KeptAvatar[];
  // Why the account is not connected, where it is not.
  missing: () => string | null;
}

type Body = Record<string, unknown>;

const isNumber = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);
const isText = (value: unknown): value is string => typeof value === "string";

function json(body: unknown, status = 200): Response {
  return Response.json(body, { status });
}

function failure(code: string, error: string, status: number): Response {
  return json({ code, error }, status);
}

// The message a send in the owner's voice answers, as it crosses: null where none is named, undefined where
// what was named is not a number.
function replyOf(value: unknown): number | null | undefined {
  if (value === null || value === undefined) return null;

  return isNumber(value) ? value : undefined;
}

// The markup a send in the owner's voice carries: null where none is given, undefined where what was given
// is not text.
function markupOf(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;

  return isText(value) ? value : undefined;
}

// A send in the owner's voice carried, a refusal of the account answered by its name.
async function asOwner(send: () => Promise<Body>): Promise<Response> {
  try {
    return json(await send());
  } catch (error) {
    const refused = ownerRefusalName(error);

    if (refused === null) throw error;

    return json({ refused, error: (error as Error).message });
  }
}

function routes(deps: RoutesDeps): Record<string, (body: Body) => Promise<Response>> {
  return {
    "/reader/picture": async (body) => {
      if (!isNumber(body.chatId) || !isNumber(body.messageId)) return failure("bad-request", "chatId and messageId (numbers) are required", 400);

      const sentAt = isNumber(body.sentAt) ? body.sentAt : null;

      return json(await fetchPicture(deps.client(), deps.peers, { chatId: body.chatId, messageId: body.messageId, sentAt }));
    },

    "/reader/file": async (body) => {
      if (!isNumber(body.chatId) || !isNumber(body.messageId) || !isNumber(body.sentAt)) {
        return failure("bad-request", "chatId, messageId and sentAt (numbers) are required", 400);
      }

      return json(await fetchFile(deps.client(), deps.peers, { chatId: body.chatId, messageId: body.messageId, sentAt: body.sentAt }));
    },

    "/owner/say": async (body) => {
      const replyToMessageId = replyOf(body.replyToMessageId);
      const markup = markupOf(body.markup);

      if (!isNumber(body.chatId) || !isText(body.text) || replyToMessageId === undefined || markup === undefined) {
        return failure("bad-request", "chatId (number), text (string), a markup or null and a replyToMessageId or null are required", 400);
      }

      const words = { chatId: body.chatId, text: body.text, markup, silent: body.silent === true, replyToMessageId };

      return await asOwner(async () => {
        const sent = await deps.owner.say(words);

        return { messageId: sent.id, text: sent.text };
      });
    },

    "/owner/album": async (body) => {
      const replyToMessageId = replyOf(body.replyToMessageId);
      const files = Array.isArray(body.files) ? (body.files as Body[]) : null;

      const captionMarkup = markupOf(body.captionMarkup);

      if (!isNumber(body.chatId) || files === null || !files.every((file) => isText(file.name) && isText(file.mime) && isText(file.bytes)) || replyToMessageId === undefined || captionMarkup === undefined) {
        return failure("bad-request", "chatId (number), files ({ name, mime, bytes }), a captionMarkup or null and a replyToMessageId or null are required", 400);
      }

      const album = {
        chatId: body.chatId,
        files: files.map((file) => ({ name: file.name as string, mime: file.mime as string, bytes: new Uint8Array(Buffer.from(file.bytes as string, "base64")) })),
        caption: isText(body.caption) ? body.caption : "",
        captionMarkup,
        replyToMessageId,
      };

      return await asOwner(async () => {
        const sent = await deps.owner.album(album);

        return { ids: sent.map((one) => one.id), texts: sent.map((one) => one.text) };
      });
    },
  };
}

/** The reader's listener: every route under the seam's token. */
export function readerHandler(deps: RoutesDeps): (request: Request) => Promise<Response> {
  const table = routes(deps);

  return async (request) => {
    const { pathname } = new URL(request.url);

    if (request.headers.get("authorization") !== `Bearer ${deps.token}`) {
      return failure("unauthorized", "the token is missing or wrong", 401);
    }

    if (request.method === "GET" && pathname === "/health") {
      const reason = deps.client() === null ? (deps.missing() ?? "the owner's account is not connected yet") : null;

      return json({ connected: reason === null, reason });
    }

    if (request.method === "GET" && pathname === "/reader/avatars") {
      return json({ avatars: deps.avatars() });
    }

    const route = request.method === "POST" ? table[pathname] : undefined;

    if (route === undefined) return failure("no-route", `no route ${request.method} ${pathname}`, 404);

    const body = (await request.json().catch(() => null)) as Body | null;

    if (body === null || typeof body !== "object") return failure("bad-request", "a JSON body is required", 400);

    try {
      return await route(body);
    } catch (error) {
      return failure("failed", error instanceof Error ? error.message : String(error), 500);
    }
  };
}
