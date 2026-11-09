# Reading one chat's recent messages

Use `telegram_messages` when the owner asks what one chat or person said recently and that chat is not a room
of the provider's. Name the chat by its number, or by the name or handle the owner's account shows it by.

Where a name could mean several chats or contacts, the answer lists the candidates with their numbers, names
and kinds. Ask the owner which one is meant, then call again with that number.

Arguments:

- `chat` (required): the chat's number, or the name or handle the owner's account shows it by.
- `max` (optional): how many messages at most, newest first. Fifty where absent, two hundred the most.
- `since` (optional): only messages sent at or after this moment, in epoch milliseconds.

The answer is newest first. Each line carries the moment in epoch milliseconds, the sender's name and number,
and the words as they are; a picture, file, sound or other non-text line carries its kind's placeholder. A chat
the owner's account cannot resolve is refused with the reason.

The call is read-only and stands on the owner's word alone. It sends nothing into any chat and makes no room. A
call outside the owner's turn is refused.
