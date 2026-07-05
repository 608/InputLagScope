export const APP_NAME = "InputLagScope";
export const APP_VERSION_CODE = 0x1000;

export function appVersionParts(code = APP_VERSION_CODE) {
  return {
    major: (code >> 12) & 0xf,
    minor: (code >> 8) & 0xf,
    patch: (((code >> 4) & 0xf) * 10) + (code & 0xf),
  };
}

export function formatAppVersion(code = APP_VERSION_CODE) {
  const { major, minor, patch } = appVersionParts(code);
  return `v${major}.${minor}.${patch}`;
}

export const APP_VERSION = formatAppVersion();
export const APP_DISPLAY_NAME = `${APP_NAME} ${APP_VERSION}`;
