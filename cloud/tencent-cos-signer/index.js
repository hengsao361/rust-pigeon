"use strict";

const crypto = require("crypto");

const BUCKET = process.env.COS_BUCKET || "pg-1329186100";
const REGION = process.env.COS_REGION || "ap-nanjing";
const OBJECT_PREFIX = normalizePrefix(process.env.COS_OBJECT_PREFIX || "log/printer");
const URL_EXPIRES_SECONDS = clampInteger(process.env.URL_EXPIRES_SECONDS, 300, 60, 900);

function response(statusCode, body) {
  return {
    statusCode,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
    body: JSON.stringify(body),
  };
}

function normalizePrefix(value) {
  return String(value).replace(/^\/+|\/+$/g, "");
}

function clampInteger(value, fallback, minimum, maximum) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed)) {
    return fallback;
  }
  return Math.min(maximum, Math.max(minimum, parsed));
}

function headerValue(headers, name) {
  if (!headers || typeof headers !== "object") {
    return "";
  }
  const wanted = name.toLowerCase();
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === wanted && typeof value === "string") {
      return value;
    }
  }
  return "";
}

function safeEqual(left, right) {
  const leftBuffer = Buffer.from(left);
  const rightBuffer = Buffer.from(right);
  return (
    leftBuffer.length === rightBuffer.length &&
    crypto.timingSafeEqual(leftBuffer, rightBuffer)
  );
}

function authorized(event) {
  const expected = process.env.UPLOAD_TOKEN || "";
  if (expected.length < 32) {
    throw new Error("UPLOAD_TOKEN must contain at least 32 characters");
  }
  const authorization = headerValue(event.headers, "authorization");
  return authorization.startsWith("Bearer ") && safeEqual(authorization.slice(7), expected);
}

function requestMethod(event) {
  return String(
    event.httpMethod ||
      (event.requestContext && event.requestContext.httpMethod) ||
      ""
  ).toUpperCase();
}

function parseBody(event) {
  if (event.body && typeof event.body === "object") {
    return event.body;
  }
  if (typeof event.body !== "string") {
    throw new Error("request body is missing");
  }
  const raw = event.isBase64Encoded
    ? Buffer.from(event.body, "base64").toString("utf8")
    : event.body;
  return JSON.parse(raw);
}

function validUtcDate(year, month, day) {
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    year >= 2000 &&
    year <= 2099 &&
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
}

function validateUploadRequest(body) {
  if (!body || typeof body !== "object") {
    throw new Error("request body must be a JSON object");
  }
  const clientId = String(body.client_id || "");
  const fileName = String(body.file_name || "");
  if (!/^[0-9a-f]{32}$/.test(clientId)) {
    throw new Error("client_id is invalid");
  }
  const match = /^print-history-([0-9a-f]{32})-(\d{4})-(\d{2})-(\d{2})\.csv$/.exec(
    fileName
  );
  if (!match || match[1] !== clientId) {
    throw new Error("file_name is invalid or belongs to another client");
  }
  if (!validUtcDate(Number(match[2]), Number(match[3]), Number(match[4]))) {
    throw new Error("file_name contains an invalid UTC date");
  }
  return { clientId, fileName };
}

function createSignedPutUrl(objectKey) {
  const COS = require("cos-nodejs-sdk-v5");
  const secretId = process.env.TENCENTCLOUD_SECRETID;
  const secretKey = process.env.TENCENTCLOUD_SECRETKEY;
  const sessionToken = process.env.TENCENTCLOUD_SESSIONTOKEN;
  if (!secretId || !secretKey || !sessionToken) {
    return Promise.reject(new Error("SCF runtime role credentials are unavailable"));
  }

  const cos = new COS({ SecretId: secretId, SecretKey: secretKey });
  return new Promise((resolve, reject) => {
    cos.getObjectUrl(
      {
        Bucket: BUCKET,
        Region: REGION,
        Key: objectKey,
        Method: "PUT",
        Sign: true,
        Expires: URL_EXPIRES_SECONDS,
        Protocol: "https:",
        Query: {
          "x-cos-security-token": sessionToken,
        },
      },
      (error, data) => {
        if (error) {
          reject(error);
        } else {
          resolve(data.Url);
        }
      }
    );
  });
}

exports.main_handler = async (event) => {
  try {
    if (requestMethod(event) !== "POST") {
      return response(405, { error: "method_not_allowed" });
    }
    if (!authorized(event)) {
      return response(401, { error: "unauthorized" });
    }

    let upload;
    try {
      upload = validateUploadRequest(parseBody(event));
    } catch (error) {
      return response(400, { error: "invalid_request", message: error.message });
    }

    const objectKey = `${OBJECT_PREFIX}/${upload.clientId}/${upload.fileName}`;
    const uploadUrl = await createSignedPutUrl(objectKey);
    return response(200, {
      method: "PUT",
      upload_url: uploadUrl,
      object_key: objectKey,
      expires_in: URL_EXPIRES_SECONDS,
    });
  } catch (error) {
    console.error("Failed to create COS upload URL", error);
    return response(500, { error: "internal_error" });
  }
};

exports._test = {
  normalizePrefix,
  validateUploadRequest,
  validUtcDate,
};
