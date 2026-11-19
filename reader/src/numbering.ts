// The numbering that crosses to the service (docs/reader.md, 「Numbers」, docs/telegram.md, 「The owner's
// voice」): a supergroup or a channel numbers its messages once for everyone, so its numbers cross as they
// stand; every other room numbers them per account, where the bot account's numbers already stand on the
// receiving side, so a number the owner's account gave crosses as the provider's own for the line — offset far
// below every number the Bot API hands out, rising with the account's numbers so older still sorts first — and
// is turned back wherever a reply, an act or a fetch names one.

// Where the owner account's numbers cross from: far below any number the Bot API hands out, so none collides.
const providerNumberBase = -1_000_000_000;

/** Whether a room numbers its messages for everyone: a supergroup or a channel. */
export function numbersForEveryone(chatId: number): boolean {
  return String(chatId).startsWith("-100");
}

/** The provider's number for a line of `chatId` the owner's account gave `ownId`: it as it stands where the
 * room numbers for everyone, the account's number offset below every bot number elsewhere. */
export function providerMessageId(chatId: number, ownId: number): number {
  return numbersForEveryone(chatId) ? ownId : providerNumberBase + ownId;
}

/** The account's own number for the number `messageId` names the line by: turned back where a per-account room
 * is named by a provider number, as it stands everywhere else. */
export function accountMessageId(chatId: number, messageId: number): number {
  return !numbersForEveryone(chatId) && messageId < 0 ? messageId - providerNumberBase : messageId;
}
