"use strict";

const assert = require("assert/strict");
const { normalizePrefix, validateUploadRequest, validCalendarDate } = require("./index")._test;

const clientId = "0123456789abcdef0123456789abcdef";

assert.equal(normalizePrefix("/log/printer/"), "log/printer");
assert.equal(validCalendarDate(2024, 2, 29), true);
assert.equal(validCalendarDate(2025, 2, 29), false);

assert.deepEqual(
  validateUploadRequest({
    client_id: clientId,
    file_name: `print-history-${clientId}-2026-10-19.csv`,
  }),
  {
    clientId,
    fileName: `print-history-${clientId}-2026-10-19.csv`,
  }
);

assert.throws(() =>
  validateUploadRequest({
    client_id: clientId,
    file_name: `print-history-ffffffffffffffffffffffffffffffff-2026-10-19.csv`,
  })
);
assert.throws(() =>
  validateUploadRequest({
    client_id: clientId,
    file_name: `print-history-${clientId}-2026-02-30.csv`,
  })
);
assert.throws(() =>
  validateUploadRequest({
    client_id: clientId,
    file_name: "../../outside.csv",
  })
);

console.log("Tencent COS signer validation tests passed");
