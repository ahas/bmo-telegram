// Signs the owner's account in once and prints the session the reader is started with (docs/reader.md,
// 「Connecting」). It asks, in turn, for the phone number, the code Telegram sends and the account's password
// where one is set.

import { createInterface } from "node:readline/promises";

import { TelegramClient } from "telegram";
import { StringSession } from "telegram/sessions";

const apiId = Number.parseInt(process.env.BMO_TG_API_ID ?? "", 10);
const apiHash = process.env.BMO_TG_API_HASH ?? "";

if (Number.isNaN(apiId) || apiHash === "") {
  console.error("Set BMO_TG_API_ID and BMO_TG_API_HASH in the environment first; my.telegram.org issues them.");
  process.exit(1);
}

const rl = createInterface({ input: process.stdin, output: process.stdout });
const client = new TelegramClient(new StringSession(""), apiId, apiHash, { connectionRetries: Infinity });

await client.start({
  phoneNumber: () => rl.question("Phone number, with its country code (+...): "),
  phoneCode: () => rl.question("The code Telegram sent: "),
  password: () => rl.question("Two-step verification password (Enter where none is set): "),
  onError: (error) => {
    console.error(error);
  },
});

const saved = client.session.save() ?? "";

console.log("\nSigned in. Add the line below to the environment and restart the reader:\n");
console.log(`BMO_TG_SESSION=${saved}`);

rl.close();
await client.disconnect();
process.exit(0);
