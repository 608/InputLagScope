import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { APP_DISPLAY_NAME } from "../scripts/app-version.mjs";
import "./styles.css";

const DEFAULT_LOCALE = "ja";
const LOCALE_STORAGE_KEY = "inputlagscope-locale";
const LOCALE_OPTIONS = ["ja", "en", "zh-CN"];
const POLL_START_DELAY_MS = 3000;
const ROUTE_STORAGE_KEY = "inputlagscope-route";
const ROUTE_OPTIONS = ["direct", "tester"];
const DEFAULT_ROUTE = "direct";
const TESTER_OUTPUT_DIR = "results";
// Settle time of the target detection before the trials (retry_delay_ms),
// the same default as the headless CLI.
const TESTER_DETECT_SETTLE_MS = 5;
const TESTER_NEUTRAL_SAMPLE_MS = 1000;
const TESTER_LOG_LIMIT = 200;
const U16_MAX = 65535;

const I18N = {
  ja: {
    htmlLang: "ja",
    languageCode: "JA",
    languageName: "日本語",
    language: "言語",
    minimize: "最小化",
    maximize: "最大化",
    close: "閉じる",
    pollingRate: "ポーリングレート",
    latencyMeasurement: "遅延計測",
    connection: "接続",
    detecting: "検出中",
    reprobe: "再検出",
    inputDevice: "入力デバイス",
    unselected: "未選択",
    comPort: "COMポート",
    settings: "計測設定",
    button: "ボタン",
    axis: "スティック",
    sampleCount: "サンプル数",
    timeoutMs: "タイムアウト(ms)",
    retryDelayMs: "待機(ms)",
    buttonIndex: "インデックス",
    autoSelectHint: "-1の場合は自動選択",
    stickAxis: "インデックス",
    axisThreshold: "スティック閾値",
    autoAxisThreshold: "自動設定",
    autoAxisThresholdRunning: "設定中",
    durationSeconds: "計測時間(秒)",
    start: "開始",
    pollStartDelayHint: "計測は3秒後に開始されます",
    stop: "停止",
    devices: "デバイス",
    deviceCount: (count) => `${count}件`,
    detectingDevices: "検出中です",
    noDevices: "デバイスがありません",
    progressSamples: "進捗",
    progressSeconds: "進捗",
    status: "ステータス",
    idle: "待機中",
    running: "計測中",
    averageMs: "平均遅延(ms)",
    medianMs: "中央値(ms)",
    p05Ms: "P05(ms)",
    p95Ms: "P95(ms)",
    jitterMs: "ジッター(ms)",
    pollRateHz: "ポーリングレート(Hz)",
    samples: "サンプル数",
    averageUs: "平均間隔(us)",
    medianUs: "中央値(us)",
    p95Us: "P95(us)",
    stalls: "ストール回数",
    distributionHz: "分布(Hz)",
    binCount: (count) => `${count}区間`,
    latencyWaveform: "モニター",
    log: "ログ",
    selected: "選択中",
    noData: "データがありません",
    unknownProtocol: "不明",
    route: "コントローラーの接続先",
    routeDirect: "PC",
    routeTester: "テスター",
    tester: "テスター",
    noTesters: "テスターが見つかりません",
    attachedController: "接続中のコントローラー",
    noController: "コントローラーが接続されていません",
    interTrialMs: "試行の間隔(ms)",
    triggerPinButton: "ボタンの信号線をPA0につないでください",
    triggerPinStick: "スティックの信号線をPA1につないでください",
    incomplete: "不完全",
    errNoTester: "テスターが選択されていません。再検出してからテスターを選んでください",
    errRange: (name, min, max) => `${name}は${min}〜${max}の整数で入力してください`,
    errAxisThreshold: "スティック閾値は0より大きく2以下の値で入力してください",
    reenumerate: "再接続",
    reenumerating: "再接続中",
    exportEnumTrace: "接続ログを保存",
    exporting: "保存中",
    exportedTo: (path) => `保存先: ${path}`,
    phases: {
      idle: "待機中",
      starting: "準備中",
      monitor: "入力を検出中",
      trials: "試行中",
      polling: "計測中",
      reading: "結果を読み出し中",
      finished: "完了",
      error: "エラー",
    },
    speeds: {
      none: "なし",
      low: "Low Speed",
      full: "Full Speed",
      high: "High Speed",
      unknown: "不明",
    },
  },
  en: {
    htmlLang: "en",
    languageCode: "EN",
    languageName: "English",
    language: "Language",
    minimize: "Minimize",
    maximize: "Maximize",
    close: "Close",
    pollingRate: "Polling Rate",
    latencyMeasurement: "Latency Measurement",
    connection: "Connection",
    detecting: "Detecting",
    reprobe: "Rescan",
    inputDevice: "Input Device",
    unselected: "Not Selected",
    comPort: "COM Port",
    settings: "Measurement Settings",
    button: "Button",
    axis: "Stick",
    sampleCount: "Sample Count",
    timeoutMs: "Timeout(ms)",
    retryDelayMs: "Wait(ms)",
    buttonIndex: "Index",
    autoSelectHint: "-1 selects automatically",
    stickAxis: "Index",
    axisThreshold: "Axis Threshold",
    autoAxisThreshold: "Auto Set",
    autoAxisThresholdRunning: "Setting",
    durationSeconds: "Duration(s)",
    start: "Start",
    pollStartDelayHint: "Measurement starts after 3 seconds.",
    stop: "Stop",
    devices: "Devices",
    deviceCount: (count) => `${count} device${count === 1 ? "" : "s"}`,
    detectingDevices: "Detecting devices",
    noDevices: "No devices",
    progressSamples: "Progress",
    progressSeconds: "Progress",
    status: "Status",
    idle: "Idle",
    running: "Running",
    averageMs: "Average Latency(ms)",
    medianMs: "Median(ms)",
    p05Ms: "P05(ms)",
    p95Ms: "P95(ms)",
    jitterMs: "Jitter(ms)",
    pollRateHz: "Polling Rate(Hz)",
    samples: "Sample Count",
    averageUs: "Average Interval(us)",
    medianUs: "Median Interval(us)",
    p95Us: "P95(us)",
    stalls: "Stalls",
    distributionHz: "Distribution(Hz)",
    binCount: (count) => `${count} bins`,
    latencyWaveform: "Monitor",
    log: "Log",
    selected: "Selected",
    noData: "No data",
    unknownProtocol: "Unknown",
    route: "Controller Connected To",
    routeDirect: "PC",
    routeTester: "Tester",
    tester: "Tester",
    noTesters: "No tester found",
    attachedController: "Attached Controller",
    noController: "No controller is attached",
    interTrialMs: "Trial Interval(ms)",
    triggerPinButton: "Connect the button signal line to PA0.",
    triggerPinStick: "Connect the stick signal line to PA1.",
    incomplete: "Incomplete",
    errNoTester: "No tester is selected. Rescan and select a tester.",
    errRange: (name, min, max) => `${name} must be an integer from ${min} to ${max}.`,
    errAxisThreshold: "Axis threshold must be greater than 0 and at most 2.",
    reenumerate: "Reconnect",
    reenumerating: "Reconnecting",
    exportEnumTrace: "Save Connection Log",
    exporting: "Saving",
    exportedTo: (path) => `Saved to: ${path}`,
    phases: {
      idle: "Idle",
      starting: "Starting",
      monitor: "Detecting Input",
      trials: "Running Trials",
      polling: "Measuring",
      reading: "Reading Results",
      finished: "Finished",
      error: "Error",
    },
    speeds: {
      none: "None",
      low: "Low Speed",
      full: "Full Speed",
      high: "High Speed",
      unknown: "Unknown",
    },
  },
  "zh-CN": {
    htmlLang: "zh-CN",
    languageCode: "ZH",
    languageName: "中文",
    language: "语言",
    minimize: "最小化",
    maximize: "最大化",
    close: "关闭",
    pollingRate: "轮询率",
    latencyMeasurement: "延迟测量",
    connection: "连接",
    detecting: "检测中",
    reprobe: "重新检测",
    inputDevice: "输入设备",
    unselected: "未选择",
    comPort: "COM端口",
    settings: "测量设置",
    button: "按钮",
    axis: "摇杆",
    sampleCount: "样本数",
    timeoutMs: "超时(ms)",
    retryDelayMs: "等待(ms)",
    buttonIndex: "索引",
    autoSelectHint: "-1时自动选择",
    stickAxis: "索引",
    axisThreshold: "轴阈值",
    autoAxisThreshold: "自动设置",
    autoAxisThresholdRunning: "设置中",
    durationSeconds: "测量时间(秒)",
    start: "开始",
    pollStartDelayHint: "测量将在3秒后开始",
    stop: "停止",
    devices: "设备",
    deviceCount: (count) => `${count}个`,
    detectingDevices: "正在检测设备",
    noDevices: "没有设备",
    progressSamples: "进度",
    progressSeconds: "进度",
    status: "状态",
    idle: "待机",
    running: "测量中",
    averageMs: "平均延迟(ms)",
    medianMs: "中位数(ms)",
    p05Ms: "P05(ms)",
    p95Ms: "P95(ms)",
    jitterMs: "抖动(ms)",
    pollRateHz: "轮询率(Hz)",
    samples: "样本数",
    averageUs: "平均间隔(us)",
    medianUs: "中位间隔(us)",
    p95Us: "P95(us)",
    stalls: "停顿",
    distributionHz: "分布(Hz)",
    binCount: (count) => `${count}个区间`,
    latencyWaveform: "监视器",
    log: "日志",
    selected: "已选择",
    noData: "没有数据",
    unknownProtocol: "未知",
    route: "控制器连接到",
    routeDirect: "电脑",
    routeTester: "测试器",
    tester: "测试器",
    noTesters: "未找到测试器",
    attachedController: "已连接的控制器",
    noController: "未连接控制器",
    interTrialMs: "试验间隔(ms)",
    triggerPinButton: "请将按键信号线连接到PA0",
    triggerPinStick: "请将摇杆信号线连接到PA1",
    incomplete: "不完整",
    errNoTester: "未选择测试器。请重新检测后选择测试器",
    errRange: (name, min, max) => `${name}必须是${min}到${max}之间的整数`,
    errAxisThreshold: "轴阈值必须大于0且不超过2",
    reenumerate: "重新连接",
    reenumerating: "正在重新连接",
    exportEnumTrace: "保存连接日志",
    exporting: "正在保存",
    exportedTo: (path) => `保存位置: ${path}`,
    phases: {
      idle: "待机",
      starting: "准备中",
      monitor: "正在检测输入",
      trials: "试验中",
      polling: "测量中",
      reading: "正在读取结果",
      finished: "已完成",
      error: "错误",
    },
    speeds: {
      none: "无",
      low: "低速",
      full: "全速",
      high: "高速",
      unknown: "未知",
    },
  },
};

function normalizeLocale(locale) {
  const value = String(locale ?? "").toLowerCase();
  if (value.startsWith("zh")) return "zh-CN";
  if (value.startsWith("en")) return "en";
  if (value.startsWith("ja")) return "ja";
  return "";
}

function initialLocale() {
  try {
    const stored = normalizeLocale(window.localStorage?.getItem(LOCALE_STORAGE_KEY));
    if (stored) return stored;
  } catch {
    // Ignore storage access failures in restricted webviews.
  }
  return normalizeLocale(navigator.language) || DEFAULT_LOCALE;
}

function initialRoute() {
  try {
    const stored = window.localStorage?.getItem(ROUTE_STORAGE_KEY);
    if (ROUTE_OPTIONS.includes(stored)) return stored;
  } catch {
    // Ignore storage access failures in restricted webviews.
  }
  return DEFAULT_ROUTE;
}

const state = {
  activeTab: "poll",
  locale: initialLocale(),
  route: initialRoute(),
  testerProbe: null,
  selectedTesterId: "",
  // Log lines written by the GUI in tester mode; merged with the backend
  // status messages so that they survive re-rendering.
  testerLog: [],
  testerFormError: "",
  // Tester form values are kept as entered so that validation can report
  // them precisely.
  testerMeasure: {
    inputType: "button",
    sampleCount: "100",
    timeoutMs: "1000",
    interTrialMs: "15",
    buttonIndex: -1,
    axisIndex: -1,
    // Report ID found by the axis auto-set (not shown in the form).
    axisReportId: null,
    axisThreshold: "0.35",
  },
  testerPoll: {
    durationSeconds: "5",
  },
  testerSettings: {
    loadedFor: "",
    lastExportPath: "",
  },
  // Errors of the controller panel.
  testerPanelErrors: {
    controller: "",
  },
  // Tester command in flight: "reenumerate" or "export".
  testerAction: null,
  probe: null,
  probeRunning: false,
  selectedDeviceId: "",
  selectedSerialPort: "",
  latencyStatusTimer: null,
  pollStatusTimer: null,
  pollStartDelayTimer: null,
  pollStartDelayStartedAt: 0,
  latencyStartPending: false,
  axisThresholdCalibrating: false,
  latencyRunning: false,
  pollStartInFlight: false,
  pollStopAfterStart: false,
  pollRunning: false,
  pollStartPending: false,
  measure: {
    inputType: "button",
    sampleCount: 100,
    timeoutMs: 1000,
    retryDelayMs: 5,
    buttonIndex: -1,
    axisIndex: -1,
    axisThreshold: 0.35,
  },
  poll: {
    durationSeconds: 5,
  },
};

const app = document.querySelector("#app");

function texts() {
  return I18N[state.locale] ?? I18N[DEFAULT_LOCALE];
}

function t(key, ...args) {
  const value = texts()[key] ?? I18N[DEFAULT_LOCALE][key] ?? key;
  return typeof value === "function" ? value(...args) : value;
}

function nameOf(group, name) {
  const key = String(name ?? "");
  return texts()[group]?.[key] ?? I18N[DEFAULT_LOCALE][group]?.[key] ?? key;
}

function protocolLabel(protocol) {
  return {
    ds4: "DS4",
    dualsense: "DualSense",
    dual_sense: "DualSense",
    switch: "Switch",
    xinput: "XInput",
    x_input: "XInput",
    xusb: "XUSB",
    gip: "GIP",
    generic_hid: "Generic HID",
    unknown: t("unknownProtocol"),
  }[protocol] ?? protocol;
}

function serialPortDisplayName(port) {
  return port.display_name || port.product || port.manufacturer || "";
}

function fmtHex(value, width = 4) {
  return value == null ? "" : `0x${Number(value).toString(16).toUpperCase().padStart(width, "0")}`;
}

function fmtUsage(device) {
  const usagePage = fmtHex(device.usage_page, 2);
  const usage = fmtHex(device.usage, 2);
  return usagePage || usage ? `${usagePage}:${usage}` : "";
}

function fmtMs(value) {
  return value == null || Number.isNaN(Number(value)) ? "-" : Number(value).toFixed(3);
}

function fmtUs(value) {
  return value == null || Number.isNaN(Number(value)) ? "-" : Number(value).toFixed(2);
}

function fmtHz(value) {
  return value == null || Number.isNaN(Number(value)) ? "-" : Number(value).toFixed(1);
}

function html(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function applyLocaleAttributes() {
  document.documentElement.lang = t("htmlLang");
  document.documentElement.dataset.locale = state.locale;
  document.body.dataset.locale = state.locale;
  setButtonLabel("#win-min", t("minimize"));
  setButtonLabel("#win-max", t("maximize"));
  setButtonLabel("#win-close", t("close"));
}

function setButtonLabel(selector, label) {
  const button = document.querySelector(selector);
  if (!button) return;
  button.setAttribute("aria-label", label);
  button.setAttribute("title", label);
}

function applyAppVersion() {
  document.title = APP_DISPLAY_NAME;
  const titlebarName = document.querySelector(".titlebar-name");
  if (titlebarName) titlebarName.textContent = APP_DISPLAY_NAME;
  const titlebarMark = document.querySelector(".titlebar-mark");
  if (titlebarMark) titlebarMark.setAttribute("aria-label", APP_DISPLAY_NAME);
}

function setLocale(locale) {
  const normalized = normalizeLocale(locale) || DEFAULT_LOCALE;
  if (!LOCALE_OPTIONS.includes(normalized)) return;
  state.locale = normalized;
  try {
    window.localStorage?.setItem(LOCALE_STORAGE_KEY, normalized);
  } catch {
    // Locale persistence is optional.
  }
  render();
}

function renderLocaleName(locale) {
  const localeText = I18N[locale] ?? I18N[DEFAULT_LOCALE];
  return `<span class="locale-code">${localeText.languageCode}</span><span class="locale-name">${localeText.languageName}</span>`;
}

function render() {
  applyLocaleAttributes();
  const devices = state.probe?.devices ?? [];
  const serialPorts = state.probe?.serial_ports ?? [];
  const isProbing = state.probeRunning;
  const tester = isTesterRoute();
  const latency = state.activeTab === "latency";

  app.innerHTML = `
    ${renderTabBar()}
    <section class="shell">
      <aside class="sidebar">
        ${renderConnectionPanel(devices, serialPorts, isProbing)}
        ${tester
          ? latency ? renderTesterLatencyForm() : renderTesterPollForm()
          : latency ? renderLatencyForm() : renderPollForm()}
      </aside>

      <section class="main">
        ${tester ? renderTesterOverview(isProbing) : renderDeviceTable(devices, isProbing)}
        ${latency
          ? tester ? renderTesterLatencyMain() : renderLatencyMain()
          : tester ? renderTesterPollMain() : renderPollMain()}
      </section>
    </section>`;

  bindUi();
  if (tester) updateBusyControls();
  if (state.activeTab === "latency") {
    refreshStatus();
  } else {
    refreshPollStatus();
  }
}

function renderConnectionPanel(devices, serialPorts, isProbing) {
  return `
    <div class="panel compact">
      <div class="panel-title">
        <h2>${t("connection")}</h2>
        <button id="probeBtn" type="button" ${isProbing ? "disabled" : ""}>${isProbing ? t("detecting") : t("reprobe")}</button>
      </div>
      ${renderRouteControl(isMeasurementBusy() || isProbing)}
      ${isTesterRoute() ? renderTesterSelect(isProbing) : renderDirectConnectionFields(devices, serialPorts, isProbing)}
    </div>`;
}

function renderRouteControl(locked) {
  return `
    <div class="field">
      <span class="field-caption">${t("route")}</span>
      <div class="segmented" role="radiogroup" aria-label="${html(t("route"))}">
        ${ROUTE_OPTIONS.map((route) => `
          <label><input type="radio" name="route" value="${route}" ${state.route === route ? "checked" : ""} ${locked ? "disabled" : ""} />${t(route === "tester" ? "routeTester" : "routeDirect")}</label>`).join("")}
      </div>
    </div>`;
}

function renderTesterSelect(isProbing) {
  const testers = testerEntries();
  return `
    <label>
      <span>${t("tester")}</span>
      <select id="testerSelect">
        ${testers.length ? "" : `<option value="">${isProbing ? t("detecting") : t("noTesters")}</option>`}
        ${testers.map((entry) => `
          <option value="${html(entry.tester.uuid)}" ${entry.tester.uuid === state.selectedTesterId ? "selected" : ""}>
            ${html(testerOptionLabel(entry.tester))}
          </option>`).join("")}
      </select>
    </label>`;
}

function renderDirectConnectionFields(devices, serialPorts, isProbing) {
  return `
      <label>
        <span>${t("inputDevice")}</span>
        <select id="deviceSelect">
          <option value="">${isProbing && !devices.length ? t("detecting") : t("unselected")}</option>
          ${devices.map((device) => `
            <option value="${html(device.id)}" ${device.id === state.selectedDeviceId ? "selected" : ""}>
              ${html(protocolLabel(device.protocol))} / ${html(device.name || device.id)}
            </option>`).join("")}
        </select>
      </label>
      ${state.activeTab === "latency" ? `
        <label>
          <span>${t("comPort")}</span>
          <select id="serialSelect">
            <option value="">${t("unselected")}</option>
            ${serialPorts.map((port) => `
              <option value="${html(port.port_name)}" ${port.port_name === state.selectedSerialPort ? "selected" : ""}>
                ${html(port.port_name)}${serialPortDisplayName(port) ? ` / ${html(serialPortDisplayName(port))}` : ""}
              </option>`).join("")}
          </select>
        </label>` : ""}`;
}

function renderTabBar() {
  const tabs = [
    { value: "poll", label: t("pollingRate") },
    { value: "latency", label: t("latencyMeasurement") },
  ];
  return `
    <div class="tabbar">
      <nav class="tabs" role="tablist">
        ${tabs.map((tab) => `
          <button type="button" role="tab" class="tab ${state.activeTab === tab.value ? "is-active" : ""}"
            data-tab="${tab.value}" aria-selected="${state.activeTab === tab.value}">${tab.label}</button>`).join("")}
      </nav>
      <div class="language-control">
        <span class="language-icon" aria-hidden="true">
          <svg viewBox="0 0 20 20" width="18" height="18">
            <circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" stroke-width="1.6" />
            <path d="M3 10h14M10 3c2 2 3 4.4 3 7s-1 5-3 7M10 3c-2 2-3 4.4-3 7s1 5 3 7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
        </span>
        <details class="language-menu">
          <summary aria-label="${t("language")}" title="${t("language")}">
            <span class="language-current">${renderLocaleName(state.locale)}</span>
          </summary>
          <div class="language-options">
            ${LOCALE_OPTIONS.map((locale) => `
              <button type="button" class="language-option ${locale === state.locale ? "is-active" : ""}"
                data-locale="${locale}" aria-current="${locale === state.locale ? "true" : "false"}">
                ${renderLocaleName(locale)}
              </button>`).join("")}
          </div>
        </details>
      </div>
    </div>`;
}

function renderLatencyForm() {
  const measure = state.measure;
  const startDisabled = isMeasurementBusy();
  return `
    <form id="measureForm" class="panel compact">
      <h2>${t("settings")}</h2>
      <div class="segmented">
        <label><input type="radio" name="inputType" value="button" ${measure.inputType === "button" ? "checked" : ""} />${t("button")}</label>
        <label><input type="radio" name="inputType" value="axis" ${measure.inputType === "axis" ? "checked" : ""} />${t("axis")}</label>
      </div>
      <label><span>${t("sampleCount")}</span><input name="sampleCount" type="number" min="1" max="100000" value="${html(measure.sampleCount)}" /></label>
      <label><span>${t("timeoutMs")}</span><input name="timeoutMs" type="number" min="1" max="5000" value="${html(measure.timeoutMs)}" /></label>
      <label><span>${t("retryDelayMs")}</span><input name="retryDelayMs" type="number" min="0" max="1000" value="${html(measure.retryDelayMs)}" /></label>
      ${measure.inputType === "button" ? `
        <label>
          <span>${t("buttonIndex")}</span>
          <input name="buttonIndex" type="number" min="-1" value="${html(measure.buttonIndex)}" />
          <small class="field-hint">${t("autoSelectHint")}</small>
        </label>` : ""}
      ${measure.inputType === "axis" ? `
        <label>
          <span>${t("stickAxis")}</span>
          <input name="axisIndex" type="number" min="-1" value="${html(measure.axisIndex)}" />
          <small class="field-hint">${t("autoSelectHint")}</small>
        </label>
        <div class="field">
          <label for="axisThresholdInput">${t("axisThreshold")}</label>
          <div class="field-with-button">
            <input id="axisThresholdInput" name="axisThreshold" type="number" min="0.01" max="1" step="0.01" value="${html(measure.axisThreshold)}" />
            <button id="autoAxisThresholdBtn" type="button" ${startDisabled ? "disabled" : ""}>
              ${state.axisThresholdCalibrating ? t("autoAxisThresholdRunning") : t("autoAxisThreshold")}
            </button>
          </div>
        </div>` : ""}
      <div class="actions">
        <button id="startBtn" class="primary" type="submit" ${startDisabled ? "disabled" : ""}>${t("start")}</button>
        <button id="stopBtn" type="button">${t("stop")}</button>
      </div>
    </form>`;
}

function renderPollForm() {
  const startDisabled = isMeasurementBusy();
  return `
    <form id="pollForm" class="panel compact">
      <h2>${t("settings")}</h2>
      <label><span>${t("durationSeconds")}</span><input name="durationSeconds" type="number" min="1" max="120" value="${html(state.poll.durationSeconds)}" /></label>
      <div class="actions-block">
        <div class="actions">
          <button id="startPollBtn" class="primary ${state.pollStartPending ? "is-counting-down" : ""}" type="submit" ${startDisabled ? "disabled" : ""} ${pollStartHighlightStyle()}>
            <span>${t("start")}</span>
          </button>
          <button id="stopPollBtn" type="button">${t("stop")}</button>
        </div>
        <small class="start-delay-hint">${t("pollStartDelayHint")}</small>
      </div>
    </form>`;
}

function renderDeviceTable(devices, isProbing) {
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("devices")}</h2>
        <span>${t("deviceCount", devices.length)}</span>
      </div>
      <div class="table-wrap">
        <table>
          <thead>
            <!-- Keep these device field headers in English. They match raw device metadata names. -->
            <tr>
              <th>Protocol</th>
              <th>Name</th>
              <th>VID</th>
              <th>PID</th>
              <th>Usage</th>
              <th>Buttons</th>
              <th>Axes</th>
              <th>Report</th>
              <th>Interface</th>
            </tr>
          </thead>
          <tbody>
            ${devices.length ? devices.map((device) => `
              <tr class="${device.id === state.selectedDeviceId ? "selected" : ""}" data-device-id="${html(device.id)}">
                <td>${html(protocolLabel(device.protocol))}</td>
                <td>${html(device.name || "")}</td>
                <td>${html(fmtHex(device.vendor_id))}</td>
                <td>${html(fmtHex(device.product_id))}</td>
                <td>${html(fmtUsage(device))}</td>
                <td>${html(device.parsed_button_count ?? "")}</td>
                <td>${html(device.parsed_axis_count ?? "")}</td>
                <td>${html(device.input_report_bytes ?? "")}</td>
                <td>${html(device.interface_number ?? "")}</td>
              </tr>`).join("") : `<tr><td colspan="9" class="empty">${isProbing ? t("detectingDevices") : t("noDevices")}</td></tr>`}
          </tbody>
        </table>
      </div>
    </section>`;
}

function renderProgress(idPrefix, caption) {
  return `
    <div class="progress">
      <div class="progress-head">
        <span class="progress-caption">${caption}</span>
        <span id="${idPrefix}Text" class="progress-value">-</span>
      </div>
      <div class="progress-track">
        <div id="${idPrefix}Bar" class="progress-bar" style="width: 0%"></div>
      </div>
    </div>`;
}

function setProgress(idPrefix, percent) {
  const clamped = Math.max(0, Math.min(100, percent));
  const textEl = document.querySelector(`#${idPrefix}Text`);
  const barEl = document.querySelector(`#${idPrefix}Bar`);
  if (textEl) textEl.textContent = `${Math.round(clamped)}%`;
  if (barEl) barEl.style.width = `${clamped}%`;
}

function renderLatencyMain() {
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("status")}</h2>
        <span id="runState" class="run-state">${t("idle")}</span>
      </div>
      ${renderProgress("progress", t("progressSamples"))}
      <dl class="stats">
        <div><dt>${t("averageMs")}</dt><dd id="mean">-</dd></div>
        <div><dt>${t("medianMs")}</dt><dd id="median">-</dd></div>
        <div><dt>${t("p05Ms")}</dt><dd id="p05">-</dd></div>
        <div><dt>${t("p95Ms")}</dt><dd id="p95">-</dd></div>
        <div><dt>${t("jitterMs")}</dt><dd id="jitter">-</dd></div>
      </dl>
    </section>
    <section class="panel">
      <div class="panel-title">
        <h2>${t("latencyWaveform")}</h2>
      </div>
      <div id="latencyWaveform" class="waveform"></div>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("log")}</h2></div>
      <pre id="log" class="log"></pre>
    </section>`;
}

function renderPollMain() {
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("status")}</h2>
        <span id="pollRunState" class="run-state">${t("idle")}</span>
      </div>
      ${renderProgress("pollProgress", t("progressSeconds"))}
      <dl class="stats">
        <div><dt>${t("pollRateHz")}</dt><dd id="pollRate">-</dd></div>
        <div><dt>${t("samples")}</dt><dd id="pollSamples">0</dd></div>
        <div><dt>${t("averageUs")}</dt><dd id="pollAverage">-</dd></div>
        <div><dt>${t("medianUs")}</dt><dd id="pollMedian">-</dd></div>
        <div><dt>${t("p95Us")}</dt><dd id="pollP95">-</dd></div>
      </dl>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("distributionHz")}</h2><span id="pollDistributionHint">${t("binCount", 0)}</span></div>
      <div id="pollBins" class="bars"></div>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("log")}</h2></div>
      <pre id="log" class="log"></pre>
    </section>`;
}

// ---------------------------------------------------------------------------
// CH32H417 tester: rendering

function renderTesterFormError() {
  return `<p id="testerFormError" class="form-error" role="alert" ${state.testerFormError ? "" : "hidden"}>${html(state.testerFormError)}</p>`;
}

function renderTesterLatencyForm() {
  const measure = state.testerMeasure;
  const axis = measure.inputType === "axis";
  const indexField = axis ? "axisIndex" : "buttonIndex";
  return `
    <form id="testerMeasureForm" class="panel compact" novalidate>
      <h2>${t("settings")}</h2>
      <div class="field">
        <div class="segmented">
          <label><input type="radio" name="inputType" value="button" ${axis ? "" : "checked"} />${t("button")}</label>
          <label><input type="radio" name="inputType" value="axis" ${axis ? "checked" : ""} />${t("axis")}</label>
        </div>
        <small class="field-hint">${t(axis ? "triggerPinStick" : "triggerPinButton")}</small>
      </div>
      <label><span>${t("sampleCount")}</span><input name="sampleCount" type="number" min="1" max="100000" step="1" value="${html(measure.sampleCount)}" /></label>
      <label><span>${t("timeoutMs")}</span><input name="timeoutMs" type="number" min="1" max="${U16_MAX}" step="1" value="${html(measure.timeoutMs)}" /></label>
      <label><span>${t("interTrialMs")}</span><input name="interTrialMs" type="number" min="0" max="${U16_MAX}" step="1" value="${html(measure.interTrialMs)}" /></label>
      <label>
        <span>${axis ? t("stickAxis") : t("buttonIndex")}</span>
        <input name="${indexField}" type="number" min="-1" value="${html(measure[indexField])}" />
        <small class="field-hint">${t("autoSelectHint")}</small>
      </label>
      ${axis ? `
        <div class="field">
          <label for="axisThresholdInput">${t("axisThreshold")}</label>
          <div class="field-with-button">
            <input id="axisThresholdInput" name="axisThreshold" type="number" min="0.01" max="2" step="0.01" value="${html(measure.axisThreshold)}" />
            <button id="autoAxisThresholdBtn" type="button">
              ${state.axisThresholdCalibrating ? t("autoAxisThresholdRunning") : t("autoAxisThreshold")}
            </button>
          </div>
        </div>` : ""}
      ${renderTesterFormError()}
      <div class="actions">
        <button id="startBtn" class="primary" type="submit">${t("start")}</button>
        <button id="stopBtn" type="button">${t("stop")}</button>
      </div>
    </form>`;
}

function renderTesterPollForm() {
  const poll = state.testerPoll;
  return `
    <form id="testerPollForm" class="panel compact" novalidate>
      <h2>${t("settings")}</h2>
      <label><span>${t("durationSeconds")}</span><input name="durationSeconds" type="number" min="1" max="120" step="1" value="${html(poll.durationSeconds)}" /></label>
      ${renderTesterFormError()}
      <div class="actions-block">
        <div class="actions">
          <button id="startPollBtn" class="primary ${state.pollStartPending ? "is-counting-down" : ""}" type="submit" ${pollStartHighlightStyle()}>
            <span>${t("start")}</span>
          </button>
          <button id="stopPollBtn" type="button">${t("stop")}</button>
        </div>
        <small class="start-delay-hint">${t("pollStartDelayHint")}</small>
      </div>
    </form>`;
}

function renderTesterOverview(isProbing) {
  const entry = selectedTesterEntry();
  const controller = entry?.controller ?? null;
  let empty = t("noController");
  if (isProbing) empty = t("detectingDevices");
  else if (!entry) empty = t("noTesters");
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("attachedController")}</h2>
        ${entry ? `
          <div class="panel-actions">
            <button id="reenumerateBtn" type="button">${t("reenumerate")}</button>
            <button id="exportEnumTraceBtn" type="button">${t("exportEnumTrace")}</button>
          </div>` : ""}
      </div>
      ${renderTesterPanelError("controller")}
      ${entry && state.testerSettings.lastExportPath ? `
        <p class="panel-note">${html(t("exportedTo", state.testerSettings.lastExportPath))}</p>` : ""}
      <div class="table-wrap">
        <table class="table-static">
          <thead>
            <!-- Keep these controller field headers in English, like the device table. -->
            <tr>
              <th>Protocol</th>
              <th>Product</th>
              <th>VID</th>
              <th>PID</th>
              <th>Speed</th>
              <th>Poll period(us)</th>
            </tr>
          </thead>
          <tbody>
            ${controller ? `
              <tr>
                <td>${html(protocolLabel(controller.protocol))}</td>
                <td>${html(controller.product || "")}</td>
                <td>${html(fmtHex(controller.vendor_id))}</td>
                <td>${html(fmtHex(controller.product_id))}</td>
                <td>${html(speedLabel(controller.speed_name))}</td>
                <td>${html(fmtPeriodUs(controller.poll_period_us))}</td>
              </tr>` : `<tr><td colspan="6" class="empty">${empty}</td></tr>`}
          </tbody>
        </table>
      </div>
    </section>`;
}

function renderTesterLatencyMain() {
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("status")}</h2>
        <span id="testerRunState" class="run-state">${nameOf("phases", "idle")}</span>
      </div>
      ${renderProgress("progress", t("progressSamples"))}
      <dl class="stats">
        <div><dt>${t("averageMs")}</dt><dd id="testerMean">-</dd></div>
        <div><dt>${t("medianMs")}</dt><dd id="testerMedian">-</dd></div>
        <div><dt>${t("p05Ms")}</dt><dd id="testerP05">-</dd></div>
        <div><dt>${t("p95Ms")}</dt><dd id="testerP95">-</dd></div>
        <div><dt>${t("jitterMs")}</dt><dd id="testerJitter">-</dd></div>
      </dl>
    </section>
    <section class="panel">
      <div class="panel-title">
        <h2>${t("latencyWaveform")}</h2>
      </div>
      <div id="latencyWaveform" class="waveform"></div>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("log")}</h2></div>
      <pre id="log" class="log"></pre>
    </section>`;
}

function renderTesterPollMain() {
  return `
    <section class="panel">
      <div class="panel-title">
        <h2>${t("status")}</h2>
        <div class="badge-row">
          <span id="pollIncomplete" class="badge-warn" hidden>${t("incomplete")}</span>
          <span id="testerPollRunState" class="run-state">${nameOf("phases", "idle")}</span>
        </div>
      </div>
      ${renderProgress("pollProgress", t("progressSeconds"))}
      <dl class="stats">
        <div><dt>${t("pollRateHz")}</dt><dd id="pollRate">-</dd></div>
        <div><dt>${t("samples")}</dt><dd id="pollSamples">0</dd></div>
        <div><dt>${t("averageUs")}</dt><dd id="pollAverage">-</dd></div>
        <div><dt>${t("medianUs")}</dt><dd id="pollMedian">-</dd></div>
        <div><dt>${t("p95Us")}</dt><dd id="pollP95">-</dd></div>
      </dl>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("distributionHz")}</h2><span id="pollDistributionHint">${t("binCount", 0)}</span></div>
      <div id="pollBins" class="bars"></div>
    </section>
    <section class="panel">
      <div class="panel-title"><h2>${t("log")}</h2></div>
      <pre id="log" class="log"></pre>
    </section>`;
}

function renderTesterPanelError(key) {
  const message = state.testerPanelErrors[key];
  return `<p id="${key}PanelError" class="form-error" role="alert" ${message ? "" : "hidden"}>${html(message)}</p>`;
}

function bindUi() {
  document.querySelector("#probeBtn")?.addEventListener("click", probe);
  document.querySelectorAll(".language-option[data-locale]").forEach((option) => {
    option.addEventListener("click", () => {
      setLocale(option.dataset.locale);
    });
  });
  document.querySelector("#deviceSelect")?.addEventListener("change", (event) => {
    state.selectedDeviceId = event.target.value;
    render();
  });
  document.querySelector("#serialSelect")?.addEventListener("change", (event) => {
    state.selectedSerialPort = event.target.value;
  });
  document.querySelectorAll(".tab[data-tab]").forEach((tab) => {
    tab.addEventListener("click", () => {
      if (state.activeTab === tab.dataset.tab) return;
      state.activeTab = tab.dataset.tab;
      state.testerFormError = "";
      render();
    });
  });
  document.querySelectorAll('input[name="route"]').forEach((input) => {
    input.addEventListener("change", () => {
      setRoute(input.value);
    });
  });
  document.querySelectorAll("tr[data-device-id]").forEach((row) => {
    row.addEventListener("click", () => {
      state.selectedDeviceId = row.dataset.deviceId;
      render();
    });
  });

  const measureForm = document.querySelector("#measureForm");
  measureForm?.addEventListener("submit", startMeasurement);
  measureForm?.addEventListener("input", (event) => {
    syncMeasureForm(event.currentTarget);
  });
  measureForm?.addEventListener("change", (event) => {
    syncMeasureForm(event.currentTarget);
    if (event.target?.name === "inputType") render();
  });
  document.querySelector("#stopBtn")?.addEventListener("click", stopMeasurement);
  document.querySelector("#autoAxisThresholdBtn")?.addEventListener(
    "click",
    isTesterRoute() ? testerAutoAxisThreshold : autoAxisThreshold,
  );

  const pollForm = document.querySelector("#pollForm");
  pollForm?.addEventListener("submit", startPollTest);
  pollForm?.addEventListener("input", (event) => {
    syncPollForm(event.currentTarget);
  });
  pollForm?.addEventListener("change", (event) => {
    syncPollForm(event.currentTarget);
  });
  document.querySelector("#stopPollBtn")?.addEventListener("click", stopPollTest);

  bindTesterUi();
}

function bindTesterUi() {
  document.querySelector("#testerSelect")?.addEventListener("change", (event) => {
    selectTester(event.target.value);
  });

  const testerMeasureForm = document.querySelector("#testerMeasureForm");
  testerMeasureForm?.addEventListener("submit", startTesterMeasurement);
  testerMeasureForm?.addEventListener("input", (event) => {
    syncTesterMeasureForm(event.currentTarget);
    setTesterFormError("");
  });
  testerMeasureForm?.addEventListener("change", (event) => {
    syncTesterMeasureForm(event.currentTarget);
    if (event.target?.name === "inputType") render();
  });

  const testerPollForm = document.querySelector("#testerPollForm");
  testerPollForm?.addEventListener("submit", startTesterPollTest);
  testerPollForm?.addEventListener("input", (event) => {
    syncTesterPollForm(event.currentTarget);
    setTesterFormError("");
  });
  testerPollForm?.addEventListener("change", (event) => {
    syncTesterPollForm(event.currentTarget);
  });

  document.querySelector("#reenumerateBtn")?.addEventListener("click", reenumerateTester);
  document.querySelector("#exportEnumTraceBtn")?.addEventListener("click", exportEnumTrace);
}

async function probe() {
  if (state.probeRunning) return;
  if (isTesterRoute()) {
    await probeTesters();
    return;
  }
  state.probeRunning = true;
  render();
  try {
    applyProbeResult(await invoke("probe_devices"));
  } catch (error) {
    appendLog(`probe error: ${error}`);
  } finally {
    state.probeRunning = false;
    render();
  }
}

function applyProbeResult(probeResult) {
  state.probe = probeResult;
  const devices = state.probe.devices ?? [];
  const serialPorts = state.probe.serial_ports ?? [];
  if (state.selectedDeviceId && !devices.some((device) => device.id === state.selectedDeviceId)) {
    state.selectedDeviceId = "";
  }
  if (!state.selectedDeviceId && devices.length) {
    state.selectedDeviceId = devices[0].id;
  }
  if (state.selectedSerialPort && !serialPorts.some((port) => port.port_name === state.selectedSerialPort)) {
    state.selectedSerialPort = "";
  }
  if (!state.selectedSerialPort && serialPorts.length) {
    state.selectedSerialPort = serialPorts[0].port_name;
  }
}

function formConfig(form) {
  syncMeasureForm(form);
  const measure = state.measure;
  return measurementConfigFromState(measure);
}

function measurementConfigFromState(measure) {
  return {
    device_id: state.selectedDeviceId,
    serial_port: state.selectedSerialPort,
    baud_rate: 115200,
    sample_count: positiveInteger(measure.sampleCount, 1),
    timeout_ms: positiveInteger(measure.timeoutMs, 1),
    input_type: measure.inputType,
    button_index: integerValue(measure.buttonIndex),
    axis_index: integerValue(measure.axisIndex),
    axis_threshold: measure.axisThreshold,
    neutral_sample_ms: 1000,
    retry_delay_ms: positiveInteger(measure.retryDelayMs, 0),
    output_dir: "results",
  };
}

function autoAxisThresholdConfig(form) {
  syncMeasureForm(form);
  return {
    ...measurementConfigFromState(state.measure),
    axis_index: -1,
  };
}

function pollConfig(form) {
  syncPollForm(form);
  return {
    device_id: state.selectedDeviceId,
    duration_seconds: positiveInteger(state.poll.durationSeconds, 1),
    output_dir: "results",
  };
}

function syncMeasureForm(form) {
  if (!form) return;
  const data = new FormData(form);
  state.measure.inputType = String(data.get("inputType") ?? state.measure.inputType);
  state.measure.sampleCount = formNumber(data, "sampleCount", state.measure.sampleCount);
  state.measure.timeoutMs = formNumber(data, "timeoutMs", state.measure.timeoutMs);
  state.measure.retryDelayMs = formNumber(data, "retryDelayMs", state.measure.retryDelayMs);
  state.measure.buttonIndex = formNumber(data, "buttonIndex", state.measure.buttonIndex);
  state.measure.axisIndex = formNumber(data, "axisIndex", state.measure.axisIndex);
  state.measure.axisThreshold = formNumber(data, "axisThreshold", state.measure.axisThreshold);
}

function syncPollForm(form) {
  if (!form) return;
  const data = new FormData(form);
  state.poll.durationSeconds = formNumber(data, "durationSeconds", state.poll.durationSeconds);
}

function formNumber(data, name, fallback) {
  const value = data.get(name);
  if (value == null || value === "") return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function integerValue(value) {
  return Number.isFinite(Number(value)) ? Math.round(Number(value)) : 0;
}

function positiveInteger(value, minimum) {
  return Math.max(minimum, integerValue(value));
}

function pollStartDelayElapsedMs() {
  if (!state.pollStartDelayStartedAt) return 0;
  return Math.max(0, Math.min(POLL_START_DELAY_MS, Date.now() - state.pollStartDelayStartedAt));
}

function pollStartHighlightStyle() {
  if (!state.pollStartPending) return "";
  return `style="--start-highlight-delay: -${pollStartDelayElapsedMs()}ms; --start-highlight-duration: ${POLL_START_DELAY_MS}ms;"`;
}

function isMeasurementBusy() {
  return state.latencyStartPending
    || state.axisThresholdCalibrating
    || state.latencyRunning
    || state.pollStartPending
    || state.pollStartInFlight
    || state.pollRunning
    || (isTesterRoute() && (state.probeRunning || state.testerAction != null));
}

function updateBusyControls() {
  const busy = isMeasurementBusy();
  const startBtn = document.querySelector("#startBtn");
  if (startBtn) startBtn.disabled = busy;

  const autoAxisThresholdBtn = document.querySelector("#autoAxisThresholdBtn");
  if (autoAxisThresholdBtn) {
    autoAxisThresholdBtn.disabled = busy;
    autoAxisThresholdBtn.textContent = state.axisThresholdCalibrating
      ? t("autoAxisThresholdRunning")
      : t("autoAxisThreshold");
  }

  const startPollBtn = document.querySelector("#startPollBtn");
  if (startPollBtn) {
    startPollBtn.disabled = busy;
    startPollBtn.classList.toggle("is-counting-down", state.pollStartPending);
    if (state.pollStartPending) {
      startPollBtn.style.setProperty("--start-highlight-delay", `-${pollStartDelayElapsedMs()}ms`);
      startPollBtn.style.setProperty("--start-highlight-duration", `${POLL_START_DELAY_MS}ms`);
    } else {
      startPollBtn.style.removeProperty("--start-highlight-delay");
      startPollBtn.style.removeProperty("--start-highlight-duration");
    }
  }

  const probeBtn = document.querySelector("#probeBtn");
  if (probeBtn) probeBtn.disabled = state.probeRunning || busy;

  const routeLocked = busy || state.probeRunning;
  document.querySelectorAll('input[name="route"]').forEach((input) => {
    input.disabled = routeLocked;
  });
  if (isTesterRoute()) {
    setDisabled("#testerSelect", busy);
    updateTesterActionControls(busy);
  }
}

function updateTesterActionControls(busy) {
  const action = state.testerAction;
  for (const [selector, name, idleLabel, runningLabel] of [
    ["#reenumerateBtn", "reenumerate", "reenumerate", "reenumerating"],
    ["#exportEnumTraceBtn", "export", "exportEnumTrace", "exporting"],
  ]) {
    const button = document.querySelector(selector);
    if (!button) continue;
    button.disabled = busy;
    setText(selector, t(action === name ? runningLabel : idleLabel));
  }
}

function setDisabled(selector, disabled) {
  const element = document.querySelector(selector);
  if (element) element.disabled = disabled;
}

async function startMeasurement(event) {
  event.preventDefault();
  if (isMeasurementBusy()) return;
  state.latencyStartPending = true;
  updateBusyControls();
  renderLatencyWaveform([]);
  try {
    await invoke("start_measurement", { config: formConfig(event.currentTarget) });
    state.latencyRunning = true;
    appendLog("measurement started");
    startLatencyStatusPolling();
  } catch (error) {
    appendLog(`start error: ${error}`);
  } finally {
    state.latencyStartPending = false;
    updateBusyControls();
  }
}

async function stopMeasurement() {
  try {
    await invoke(isTesterRoute() ? "tester_stop_measurement" : "stop_measurement");
    appendLog("stop requested");
  } catch (error) {
    appendLog(`stop error: ${error}`);
  }
}

function clampAxisThreshold(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return state.measure.axisThreshold;
  return Math.max(0.01, Math.min(1, Math.round(number * 100) / 100));
}

async function autoAxisThreshold() {
  const form = document.querySelector("#measureForm");
  if (!form || isMeasurementBusy()) return;
  state.axisThresholdCalibrating = true;
  updateBusyControls();
  try {
    const result = await invoke("auto_axis_threshold", { config: autoAxisThresholdConfig(form) });
    const threshold = clampAxisThreshold(result.threshold);
    const axisIndex = Number(result.axis_index);
    state.measure.axisThreshold = threshold;
    if (Number.isFinite(axisIndex)) {
      state.measure.axisIndex = axisIndex;
    }

    const thresholdInput = form.querySelector('input[name="axisThreshold"]');
    if (thresholdInput) thresholdInput.value = threshold.toFixed(2);
    const axisIndexInput = form.querySelector('input[name="axisIndex"]');
    if (axisIndexInput && Number.isFinite(axisIndex)) axisIndexInput.value = String(axisIndex);

    appendLog(`axis threshold set: axis=${Number.isFinite(axisIndex) ? axisIndex : "-"}, threshold=${threshold.toFixed(2)}`);
  } catch (error) {
    appendLog(`axis threshold error: ${error}`);
  } finally {
    state.axisThresholdCalibrating = false;
    updateBusyControls();
  }
}

async function startPollTest(event) {
  event.preventDefault();
  if (isMeasurementBusy()) return;
  const config = pollConfig(event.currentTarget);
  state.pollStartPending = true;
  state.pollStartDelayStartedAt = Date.now();
  updateBusyControls();
  state.pollStartDelayTimer = window.setTimeout(() => {
    void runDelayedPollTest(config);
  }, POLL_START_DELAY_MS);
}

const DIRECT_POLL_COMMANDS = { start: "start_poll_test", stop: "stop_poll_test" };
const TESTER_POLL_COMMANDS = { start: "tester_start_poll_test", stop: "tester_stop_poll_test" };

async function runDelayedPollTest(config, commands = DIRECT_POLL_COMMANDS) {
  state.pollStartDelayTimer = null;
  state.pollStartPending = false;
  state.pollStartInFlight = true;
  updateBusyControls();
  try {
    await invoke(commands.start, { config });
    state.pollRunning = true;
    appendLog("polling rate test started");
    startPollStatusPolling();
    if (state.pollStopAfterStart) {
      try {
        await invoke(commands.stop);
        appendLog("stop requested");
      } catch (error) {
        appendLog(`stop error: ${error}`);
      }
    }
  } catch (error) {
    appendLog(`start error: ${error}`);
  } finally {
    state.pollStartInFlight = false;
    state.pollStartDelayStartedAt = 0;
    state.pollStopAfterStart = false;
    updateBusyControls();
  }
}

function cancelPollStartDelay() {
  if (!state.pollStartDelayTimer) return false;
  window.clearTimeout(state.pollStartDelayTimer);
  state.pollStartDelayTimer = null;
  state.pollStartPending = false;
  state.pollStartDelayStartedAt = 0;
  updateBusyControls();
  return true;
}

async function stopPollTest() {
  if (cancelPollStartDelay()) {
    appendLog("polling rate start canceled");
    return;
  }
  if (state.pollStartInFlight) {
    state.pollStopAfterStart = true;
    appendLog("stop requested");
    return;
  }
  try {
    await invoke(isTesterRoute() ? TESTER_POLL_COMMANDS.stop : DIRECT_POLL_COMMANDS.stop);
    appendLog("stop requested");
  } catch (error) {
    appendLog(`stop error: ${error}`);
  }
}

function startLatencyStatusPolling() {
  if (state.latencyStatusTimer) window.clearInterval(state.latencyStatusTimer);
  state.latencyStatusTimer = window.setInterval(refreshStatus, 200);
  refreshStatus();
}

function startPollStatusPolling() {
  if (state.pollStatusTimer) window.clearInterval(state.pollStatusTimer);
  state.pollStatusTimer = window.setInterval(refreshPollStatus, 200);
  refreshPollStatus();
}

async function refreshStatus() {
  if (state.activeTab !== "latency" && !state.latencyStatusTimer) return;
  const tester = isTesterRoute();
  try {
    const status = await invoke(tester ? "tester_measurement_status" : "measurement_status");
    if (tester !== isTesterRoute()) return;
    if (tester) {
      updateTesterStatus(status);
    } else {
      updateStatus(status);
    }
    if (!status.running && state.latencyStatusTimer) {
      window.clearInterval(state.latencyStatusTimer);
      state.latencyStatusTimer = null;
    }
  } catch (error) {
    appendLog(`status error: ${error}`);
  }
}

async function refreshPollStatus() {
  if (state.activeTab !== "poll" && !state.pollStatusTimer) return;
  const tester = isTesterRoute();
  try {
    const status = await invoke(tester ? "tester_poll_status" : "poll_test_status");
    if (tester !== isTesterRoute()) return;
    if (tester) {
      updateTesterPollStatus(status);
    } else {
      updatePollStatus(status);
    }
    if (!status.running && state.pollStatusTimer) {
      window.clearInterval(state.pollStatusTimer);
      state.pollStatusTimer = null;
    }
  } catch (error) {
    appendLog(`status error: ${error}`);
  }
}

function updateStatus(status) {
  state.latencyRunning = Boolean(status.running);
  updateBusyControls();
  const runState = document.querySelector("#runState");
  if (!runState) return;
  runState.textContent = status.running ? t("running") : t("idle");
  runState.classList.toggle("is-running", Boolean(status.running));
  const requested = Number(status.requested_samples) || 0;
  const completed = Number(status.completed_samples) || 0;
  setProgress("progress", requested > 0 ? (completed / requested) * 100 : 0);
  document.querySelector("#mean").textContent = fmtMs(status.summary?.average_ms);
  document.querySelector("#median").textContent = fmtMs(status.summary?.median_ms);
  document.querySelector("#p05").textContent = fmtMs(status.summary?.p05_ms);
  document.querySelector("#p95").textContent = fmtMs(status.summary?.p95_ms);
  document.querySelector("#jitter").textContent = fmtMs(status.summary?.jitter_ms);
  document.querySelector("#log").textContent = (status.messages ?? []).join("\n");
  renderLatencyWaveform(status.latency_series ?? []);

  updateBusyControls();
}

function updatePollStatus(status) {
  state.pollRunning = Boolean(status.running);
  updateBusyControls();
  const runState = document.querySelector("#pollRunState");
  if (!runState) return;
  const summary = status.summary ?? {};
  const duration = Number(status.requested_duration_seconds) || 0;
  const rawElapsed = (status.elapsed_ms ?? 0) / 1000;
  const elapsedSeconds = duration > 0 ? Math.min(rawElapsed, duration) : rawElapsed;

  runState.textContent = status.running ? t("running") : t("idle");
  runState.classList.toggle("is-running", Boolean(status.running));
  setProgress("pollProgress", duration > 0 ? (elapsedSeconds / duration) * 100 : 0);
  document.querySelector("#pollRate").textContent = fmtHz(summary.poll_rate_hz);
  document.querySelector("#pollSamples").textContent = String(summary.sample_count ?? 0);
  document.querySelector("#pollAverage").textContent = fmtUs(summary.average_interval_us);
  document.querySelector("#pollMedian").textContent = fmtUs(summary.median_interval_us);
  document.querySelector("#pollP95").textContent = fmtUs(summary.p95_interval_us);
  document.querySelector("#log").textContent = (status.messages ?? []).join("\n");
  renderBins(status.bins ?? []);

  updateBusyControls();
}

function renderBins(bins) {
  const container = document.querySelector("#pollBins");
  const hint = document.querySelector("#pollDistributionHint");
  renderBarRows(container, hint, bins);
}

function renderBarRows(container, hint, bins) {
  if (!container) return;
  if (hint) hint.textContent = t("binCount", bins.length);
  const maxCount = Math.max(1, ...bins.map((bin) => Number(bin.count ?? 0)));
  container.innerHTML = bins.length ? bins.map((bin) => {
    const width = Math.max(1, Math.round(100 * Number(bin.count ?? 0) / maxCount));
    return `
      <div class="bar-row">
        <span class="bar-label">${html(bin.label)}</span>
        <span class="bar-track"><span class="bar-fill" style="width: ${width}%"></span></span>
        <span class="bar-value">${Number(bin.percent ?? 0).toFixed(1)}%</span>
      </div>`;
  }).join("") : `<div class="empty bars-empty">${t("noData")}</div>`;
}

const WAVEFORM_MAX_POINTS = 20;
const WAVEFORM_MINOR_US = 200;
const WAVEFORM_MAJOR_US = 400;
const WAVEFORM_RANGE_STEP_US = 2000;

// Keep the chart range stable in 2000us steps.
function waveformRangeUs(maxUs) {
  const safeMax = Number.isFinite(maxUs) ? maxUs : 0;
  let range = WAVEFORM_RANGE_STEP_US;
  while (safeMax >= range) range += WAVEFORM_RANGE_STEP_US;
  return range;
}

function renderLatencyWaveform(series) {
  const container = document.querySelector("#latencyWaveform");
  if (!container) return;

  const data = (Array.isArray(series) ? series : [])
    .filter((value) => Number.isFinite(value))
    .slice(-WAVEFORM_MAX_POINTS);
  if (!data.length) {
    container.innerHTML = `<div class="waveform-empty">${t("noData")}</div>`;
    return;
  }

  const width = 1000;
  const height = 1000;
  const yMaxUs = waveformRangeUs(Math.max(...data) * 1000);
  const lastIndex = data.length - 1;
  const points = data.map((value, index) => {
    const x = lastIndex > 0 ? (index / lastIndex) * width : width / 2;
    const y = height - Math.max(0, Math.min(1, (value * 1000) / yMaxUs)) * height;
    return [x, y];
  });
  if (points.length === 1) {
    const [, y] = points[0];
    points[0] = [0, y];
    points.push([width, y]);
  }

  const linePath = points
    .map(([x, y], index) => `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`)
    .join(" ");
  const firstX = points[0][0].toFixed(2);
  const lastX = points[points.length - 1][0].toFixed(2);
  const areaPath = `${linePath} L${lastX} ${height} L${firstX} ${height} Z`;

  const axisLabels = [];
  const gridLines = [];
  for (let us = 0; us <= yMaxUs; us += WAVEFORM_MINOR_US) {
    const pct = (1 - us / yMaxUs) * 100;
    const isMajor = us % WAVEFORM_MAJOR_US === 0;
    gridLines.push(`<i class="${isMajor ? "waveform-major" : ""}" style="top: ${pct}%"></i>`);
    if (isMajor) {
      axisLabels.push(`<span style="top: ${pct}%">${us}</span>`);
    }
  }

  container.innerHTML = `
    <div class="waveform-axis">${axisLabels.join("")}</div>
    <div class="waveform-plot-wrap">
      <div class="waveform-grid">${gridLines.join("")}</div>
      <svg class="waveform-plot" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" aria-hidden="true">
        <path class="waveform-area" d="${areaPath}" />
        <path class="waveform-line" d="${linePath}" vector-effect="non-scaling-stroke" />
      </svg>
    </div>`;
}

// ---------------------------------------------------------------------------
// CH32H417 tester: selection, commands and status

function isTesterRoute() {
  return state.route === "tester";
}

function testerEntries() {
  return state.testerProbe?.testers ?? [];
}

function selectedTesterEntry() {
  if (!state.selectedTesterId) return null;
  return testerEntries().find((entry) => entry.tester?.uuid === state.selectedTesterId) ?? null;
}

function testerOptionLabel(info) {
  return `${info.port_name} / ${String(info.uuid ?? "").slice(0, 8)} / v${info.firmware_version}`;
}

function speedLabel(name) {
  return nameOf("speeds", name || "unknown");
}

function fmtPeriodUs(value) {
  const number = Number(value);
  return value == null || !Number.isFinite(number) ? "" : String(Number(number.toFixed(3)));
}

function setText(selector, text) {
  const element = document.querySelector(selector);
  if (element && element.textContent !== text) element.textContent = text;
}

function setRoute(route) {
  if (!ROUTE_OPTIONS.includes(route) || route === state.route) return;
  if (isMeasurementBusy() || state.probeRunning) {
    render();
    return;
  }
  state.route = route;
  state.testerFormError = "";
  try {
    window.localStorage?.setItem(ROUTE_STORAGE_KEY, route);
  } catch {
    // Route persistence is optional.
  }
  render();
  if (route === "tester" ? !state.testerProbe : !state.probe) void probe();
}

function selectTester(uuid) {
  if (uuid === state.selectedTesterId) return;
  state.selectedTesterId = uuid;
  render();
}

async function probeTesters() {
  state.probeRunning = true;
  render();
  try {
    const result = await invoke("tester_probe");
    applyTesterProbeResult(result);
    logTesterProbeResult(result);
  } catch (error) {
    appendLog(`tester probe error: ${error}`);
  } finally {
    state.probeRunning = false;
    render();
  }
}

function applyTesterProbeResult(result) {
  state.testerProbe = result ?? { testers: [], errors: [] };
  const testers = testerEntries();
  const previous = state.selectedTesterId;
  if (previous && !testers.some((entry) => entry.tester.uuid === previous)) {
    state.selectedTesterId = "";
  }
  if (!state.selectedTesterId && testers.length) {
    state.selectedTesterId = testers[0].tester.uuid;
  }
}

function logTesterProbeResult(result) {
  for (const error of result?.errors ?? []) {
    appendLog(`tester probe: ${error.port_name}: ${error.error}`);
  }
  const testers = result?.testers ?? [];
  for (const entry of testers) {
    const info = entry.tester;
    appendLog(`tester found: ${info.port_name} ${info.uuid} firmware ${info.firmware_version} (${info.build})`);
    for (const warning of entry.warnings ?? []) {
      appendLog(`tester ${info.port_name}: ${warning}`);
    }
  }
  if (!testers.length) appendLog("tester probe: no tester found");
}

const TESTER_MEASURE_TEXT_FIELDS = [
  "sampleCount",
  "timeoutMs",
  "interTrialMs",
  "axisThreshold",
];

function syncTesterMeasureForm(form) {
  if (!form) return;
  const data = new FormData(form);
  const measure = state.testerMeasure;
  const inputType = data.get("inputType");
  if (inputType === "button" || inputType === "axis") measure.inputType = inputType;
  for (const name of TESTER_MEASURE_TEXT_FIELDS) {
    if (data.has(name)) measure[name] = String(data.get(name)).trim();
  }
  measure.buttonIndex = formNumber(data, "buttonIndex", measure.buttonIndex);
  const axisIndex = formNumber(data, "axisIndex", measure.axisIndex);
  if (axisIndex !== measure.axisIndex) measure.axisReportId = null;
  measure.axisIndex = axisIndex;
}

function syncTesterPollForm(form) {
  if (!form) return;
  const data = new FormData(form);
  if (data.has("durationSeconds")) {
    state.testerPoll.durationSeconds = String(data.get("durationSeconds")).trim();
  }
}

function parseIntegerText(text, min, max) {
  const value = String(text ?? "").trim();
  if (!/^-?\d+$/.test(value)) return null;
  const number = Number(value);
  return Number.isSafeInteger(number) && number >= min && number <= max ? number : null;
}

function validAxisThreshold(text) {
  const value = String(text ?? "").trim();
  const number = Number(value);
  return value !== "" && Number.isFinite(number) && number > 0 && number <= 2 ? number : null;
}

// Builds TesterMeasurementConfig; the limits follow validate_config in
// src-tauri/src/tester/measure.rs and PC_PROTOCOL.txt 4.11. The trigger pin
// follows the input type (button = PA0, stick = PA1) in the backend, and
// the other TRIAL_START parameters use the backend defaults.
function buildTesterMeasurementConfig({ forAutoSet = false } = {}) {
  const measure = state.testerMeasure;
  const fail = (error) => ({ error });
  if (!selectedTesterEntry()) return fail(t("errNoTester"));

  const values = {};
  for (const [key, label, min, max] of [
    ["sampleCount", t("sampleCount"), 1, 100000],
    ["timeoutMs", t("timeoutMs"), 1, U16_MAX],
    ["interTrialMs", t("interTrialMs"), 0, U16_MAX],
  ]) {
    const value = parseIntegerText(measure[key], min, max);
    if (value == null) return fail(t("errRange", label, min, max));
    values[key] = value;
  }

  const axis = measure.inputType === "axis";

  let axisThreshold = validAxisThreshold(measure.axisThreshold);
  if (axis && !forAutoSet && axisThreshold == null) return fail(t("errAxisThreshold"));
  // Unused by the backend in this case, but the field is required.
  if (axisThreshold == null) axisThreshold = 0.35;

  return {
    config: {
      tester: state.selectedTesterId,
      sample_count: values.sampleCount,
      timeout_ms: values.timeoutMs,
      input_type: measure.inputType,
      button_index: axis ? -1 : integerValue(measure.buttonIndex),
      // Auto-set searches every axis.
      axis_index: axis && !forAutoSet ? integerValue(measure.axisIndex) : -1,
      axis_threshold: axisThreshold,
      neutral_sample_ms: TESTER_NEUTRAL_SAMPLE_MS,
      retry_delay_ms: TESTER_DETECT_SETTLE_MS,
      inter_trial_ms: values.interTrialMs,
      output_dir: TESTER_OUTPUT_DIR,
      interface_number: null,
      report_id: axis && !forAutoSet ? measure.axisReportId : null,
    },
  };
}

function buildTesterPollConfig() {
  if (!selectedTesterEntry()) return { error: t("errNoTester") };
  const duration = parseIntegerText(state.testerPoll.durationSeconds, 1, 120);
  if (duration == null) return { error: t("errRange", t("durationSeconds"), 1, 120) };
  return {
    config: {
      tester: state.selectedTesterId,
      duration_seconds: duration,
      output_dir: TESTER_OUTPUT_DIR,
      interface_number: null,
    },
  };
}

function setTesterFormError(message) {
  if (state.testerFormError === message) return;
  state.testerFormError = message;
  const element = document.querySelector("#testerFormError");
  if (element) {
    element.textContent = message;
    element.hidden = !message;
  }
}

function reportTesterFormError(result) {
  setTesterFormError(result.error);
  appendLog(result.error);
}

async function startTesterMeasurement(event) {
  event.preventDefault();
  if (isMeasurementBusy()) return;
  syncTesterMeasureForm(event.currentTarget);
  const result = buildTesterMeasurementConfig();
  if (result.error) {
    reportTesterFormError(result);
    return;
  }
  setTesterFormError("");
  state.latencyStartPending = true;
  updateBusyControls();
  renderLatencyWaveform([]);
  try {
    await invoke("tester_start_measurement", { config: result.config });
    state.latencyRunning = true;
    appendLog("tester measurement started");
    startLatencyStatusPolling();
  } catch (error) {
    appendLog(`start error: ${error}`);
  } finally {
    state.latencyStartPending = false;
    updateBusyControls();
  }
}

function clampTesterAxisThreshold(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return validAxisThreshold(state.testerMeasure.axisThreshold) ?? 0.35;
  return Math.max(0.01, Math.min(2, Math.round(number * 100) / 100));
}

async function testerAutoAxisThreshold() {
  const form = document.querySelector("#testerMeasureForm");
  if (!form || isMeasurementBusy()) return;
  syncTesterMeasureForm(form);
  const result = buildTesterMeasurementConfig({ forAutoSet: true });
  if (result.error) {
    reportTesterFormError(result);
    return;
  }
  setTesterFormError("");
  state.axisThresholdCalibrating = true;
  updateBusyControls();
  try {
    const calibration = await invoke("tester_auto_axis_threshold", { config: result.config });
    const measure = state.testerMeasure;
    const threshold = clampTesterAxisThreshold(calibration.threshold);
    const axisIndex = Number(calibration.axis_index);
    const reportId = calibration.report_id == null ? null : Number(calibration.report_id);
    measure.axisThreshold = threshold.toFixed(2);
    if (Number.isInteger(axisIndex)) {
      measure.axisIndex = axisIndex;
      measure.axisReportId = reportId;
    }

    const thresholdInput = form.querySelector('input[name="axisThreshold"]');
    if (thresholdInput) thresholdInput.value = measure.axisThreshold;
    const axisIndexInput = form.querySelector('input[name="axisIndex"]');
    if (axisIndexInput) axisIndexInput.value = String(measure.axisIndex);

    appendLog(`axis threshold set: ${reportId == null ? "" : `report_id=${reportId}, `}axis=${Number.isInteger(axisIndex) ? axisIndex : "-"}, threshold=${measure.axisThreshold}`);
  } catch (error) {
    appendLog(`axis threshold error: ${error}`);
  } finally {
    state.axisThresholdCalibrating = false;
    updateBusyControls();
  }
}

function startTesterPollTest(event) {
  event.preventDefault();
  if (isMeasurementBusy()) return;
  syncTesterPollForm(event.currentTarget);
  const result = buildTesterPollConfig();
  if (result.error) {
    reportTesterFormError(result);
    return;
  }
  setTesterFormError("");
  state.pollStartPending = true;
  state.pollStartDelayStartedAt = Date.now();
  updateBusyControls();
  state.pollStartDelayTimer = window.setTimeout(() => {
    void runDelayedPollTest(result.config, TESTER_POLL_COMMANDS);
  }, POLL_START_DELAY_MS);
}

// ---------------------------------------------------------------------------
// CH32H417 tester: host profile, calibration, re-enumeration and trace

function setTesterPanelError(key, message) {
  if (state.testerPanelErrors[key] === message) return;
  state.testerPanelErrors[key] = message;
  const element = document.querySelector(`#${key}PanelError`);
  if (element) {
    element.textContent = message;
    element.hidden = !message;
  }
}

function reportTesterPanelError(key, logPrefix, error) {
  setTesterPanelError(key, String(error));
  appendLog(`${logPrefix}: ${error}`);
}

function clearTesterPanelErrors() {
  for (const key of Object.keys(state.testerPanelErrors)) state.testerPanelErrors[key] = "";
}

function controllerSummary(controller) {
  if (!controller) return "no controller";
  const product = controller.product ? ` ${controller.product}` : "";
  return `${fmtHex(controller.vendor_id)}:${fmtHex(controller.product_id)}${product} (${controller.speed_name}, generation ${controller.device_generation})`;
}

async function reenumerateTester() {
  if (isMeasurementBusy()) return;
  const uuid = state.selectedTesterId;
  if (!uuid) {
    reportTesterPanelError("controller", "re-enumeration error", t("errNoTester"));
    return;
  }
  setTesterPanelError("controller", "");
  state.testerAction = "reenumerate";
  updateBusyControls();
  appendLog("re-enumeration requested");
  try {
    const controller = await invoke("tester_reenumerate", { tester: uuid });
    const entry = testerEntries().find((item) => item.tester?.uuid === uuid);
    if (entry) entry.controller = controller ?? null;
    appendLog(`re-enumeration finished: ${controllerSummary(controller)}`);
  } catch (error) {
    reportTesterPanelError("controller", "re-enumeration error", error);
  } finally {
    state.testerAction = null;
    render();
  }
}

// The trace is written under TESTER_OUTPUT_DIR like the measurement
// results; the backend returns the absolute paths.
async function exportEnumTrace() {
  if (isMeasurementBusy()) return;
  const uuid = state.selectedTesterId;
  if (!uuid) {
    reportTesterPanelError("controller", "enumeration trace error", t("errNoTester"));
    return;
  }
  setTesterPanelError("controller", "");
  state.testerAction = "export";
  updateBusyControls();
  try {
    const result = await invoke("tester_export_enum_trace", {
      tester: uuid,
      outputDir: TESTER_OUTPUT_DIR,
    });
    if (state.testerSettings.loadedFor === uuid) state.testerSettings.lastExportPath = result.json_path;
    appendLog(`enumeration trace exported: ${result.json_path}`);
    appendLog(`enumeration trace: ${result.record_count} records, ${result.dropped_records} dropped, generation ${result.device_generation}, ${result.byte_length} bytes, bin ${result.bin_path}`);
    if (result.parse_error) appendLog(`enumeration trace parse error: ${result.parse_error}`);
  } catch (error) {
    reportTesterPanelError("controller", "enumeration trace error", error);
  } finally {
    state.testerAction = null;
    render();
  }
}

function setRunStateBadge(element, status) {
  const phase = status?.phase || "idle";
  const running = Boolean(status?.running);
  element.textContent = nameOf("phases", phase);
  element.classList.toggle("is-running", running);
  element.classList.toggle("is-error", !running && phase === "error");
}

function updateTesterStatus(status) {
  state.latencyRunning = Boolean(status.running);
  updateBusyControls();
  const runState = document.querySelector("#testerRunState");
  if (!runState) return;
  setRunStateBadge(runState, status);

  // While trials run only the summary is updated; the full statistics
  // arrive when the run is finished.
  const stats = status.stats ?? null;
  const latency = stats?.latency ?? null;
  const summary = status.summary ?? {};
  const requested = Number(status.requested_samples) || 0;
  const received = Number(status.completed_samples) || 0;
  setProgress("progress", requested > 0 ? (received / requested) * 100 : 0);
  setText("#testerMean", fmtMs(latency ? latency.mean_ms : summary.average_ms));
  setText("#testerMedian", fmtMs(latency ? latency.p50_ms : summary.median_ms));
  setText("#testerP05", fmtMs(latency ? latency.p05_ms : summary.p05_ms));
  setText("#testerP95", fmtMs(latency ? latency.p95_ms : summary.p95_ms));
  setText("#testerJitter", fmtMs(latency ? latency.std_dev_ms : summary.jitter_ms));
  setTesterLog(status.messages);
  renderLatencyWaveform(status.latency_series ?? []);

  updateBusyControls();
}

function updateTesterPollStatus(status) {
  state.pollRunning = Boolean(status.running);
  updateBusyControls();
  const runState = document.querySelector("#testerPollRunState");
  if (!runState) return;
  setRunStateBadge(runState, status);
  const summary = status.summary ?? {};
  const duration = Number(status.requested_duration_seconds) || 0;
  const rawElapsed = (status.elapsed_ms ?? 0) / 1000;
  const elapsedSeconds = duration > 0 ? Math.min(rawElapsed, duration) : rawElapsed;

  setProgress("pollProgress", duration > 0 ? (elapsedSeconds / duration) * 100 : 0);
  setText("#pollRate", fmtHz(summary.poll_rate_hz));
  setText("#pollSamples", String(summary.sample_count ?? 0));
  setText("#pollAverage", fmtUs(summary.average_interval_us));
  setText("#pollMedian", fmtUs(summary.median_interval_us));
  setText("#pollP95", fmtUs(summary.p95_interval_us));
  const incomplete = document.querySelector("#pollIncomplete");
  if (incomplete) {
    incomplete.hidden = !status.incomplete;
    incomplete.title = (status.stats?.incomplete_reasons ?? []).join("\n");
  }
  setTesterLog(status.messages);
  renderBins(status.bins ?? []);

  updateBusyControls();
}

function logLineTime(line) {
  const match = /^\[(\d{2}:\d{2}:\d{2})\]/.exec(String(line));
  return match ? match[1] : "";
}

// Both lists are in time order; the backend lines go first within the
// same second.
function mergeLogLines(backend, local) {
  const merged = [];
  let backendIndex = 0;
  let localIndex = 0;
  while (backendIndex < backend.length && localIndex < local.length) {
    if (logLineTime(local[localIndex]) < logLineTime(backend[backendIndex])) {
      merged.push(local[localIndex++]);
    } else {
      merged.push(backend[backendIndex++]);
    }
  }
  return merged.concat(backend.slice(backendIndex), local.slice(localIndex));
}

function setTesterLog(messages) {
  const log = document.querySelector("#log");
  if (!log) return;
  const text = mergeLogLines(Array.isArray(messages) ? messages : [], state.testerLog).join("\n");
  if (log.textContent !== text) log.textContent = text;
}

function logTimestamp() {
  const now = new Date();
  const pad = (value) => String(value).padStart(2, "0");
  return `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}

function appendLog(line) {
  const entry = `[${logTimestamp()}] ${line}`;
  if (isTesterRoute()) {
    state.testerLog.push(entry);
    if (state.testerLog.length > TESTER_LOG_LIMIT) {
      state.testerLog.splice(0, state.testerLog.length - TESTER_LOG_LIMIT);
    }
  }
  const log = document.querySelector("#log");
  if (!log) return;
  log.textContent = log.textContent ? `${log.textContent}\n${entry}` : entry;
}

function setupTitlebar() {
  const appWindow = getCurrentWindow();
  document.querySelector("#win-min")?.addEventListener("click", () => appWindow.minimize());
  document.querySelector("#win-max")?.addEventListener("click", () => appWindow.toggleMaximize());
  document.querySelector("#win-close")?.addEventListener("click", () => appWindow.close());
}

function setupUiScale() {
  const styles = getComputedStyle(document.documentElement);
  const baseWidth = Number.parseFloat(styles.getPropertyValue("--ui-base-width")) || 980;
  const baseHeight = Number.parseFloat(styles.getPropertyValue("--ui-base-height")) || 680;
  const root = document.querySelector(".ui-scale-root");
  const update = () => {
    const scale = Math.min(1, window.innerWidth / baseWidth, window.innerHeight / baseHeight);
    if (scale > 0) {
      document.documentElement.style.setProperty("--ui-scale", String(scale));
      if (root) {
        root.style.width = `${window.innerWidth / scale}px`;
        root.style.height = `${window.innerHeight / scale}px`;
      }
    }
  };

  update();
  window.addEventListener("resize", update);
  if (window.visualViewport) {
    window.visualViewport.addEventListener("resize", update);
  }
}

setupUiScale();
setupTitlebar();
applyAppVersion();
render();
probe();
