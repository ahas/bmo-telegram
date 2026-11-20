import { expect, test } from "bun:test";

import { readerSettings } from "../src/config";

// The reader's configuration (docs/boundary-api.md, 「Configuration」).

const required = { BMO_OWNER_ID: "42", BMO_TELEGRAM_TOKEN: "seam", BMO_TELEGRAM_DATA: "/data" };

test("the account is kept online unless the switch says false, and a switch neither true nor false is refused", () => {
  expect(readerSettings(required).online).toBe(true);
  expect(readerSettings({ ...required, BMO_TELEGRAM_READER_ONLINE: "true" }).online).toBe(true);
  expect(readerSettings({ ...required, BMO_TELEGRAM_READER_ONLINE: "false" }).online).toBe(false);
  expect(() => readerSettings({ ...required, BMO_TELEGRAM_READER_ONLINE: "yes" })).toThrow("BMO_TELEGRAM_READER_ONLINE must be true or false");
});
