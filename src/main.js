import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { APP_DISPLAY_NAME } from "../scripts/app-version.mjs";
import "./styles.css";

const DEFAULT_LOCALE = "ja";
const LOCALE_STORAGE_KEY = "inputlagscope-locale";
const LOCALE_OPTIONS = ["ja", "en", "zh-CN"];
const POLL_START_DELAY_MS = 3000;

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
    latencyWaveform: "ビジュアライザー",
    log: "ログ",
    selected: "選択中",
    noData: "データがありません",
    unknownProtocol: "不明",
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
    latencyWaveform: "Visualizer",
    log: "Log",
    selected: "Selected",
    noData: "No data",
    unknownProtocol: "Unknown",
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
    latencyWaveform: "可视化",
    log: "日志",
    selected: "已选择",
    noData: "没有数据",
    unknownProtocol: "未知",
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

const state = {
  activeTab: "poll",
  locale: initialLocale(),
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

function protocolLabel(protocol) {
  return {
    ds4: "DS4",
    dualsense: "DualSense",
    switch: "Switch",
    xinput: "XInput",
    x_input: "XInput",
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

  app.innerHTML = `
    ${renderTabBar()}
    <section class="shell">
      <aside class="sidebar">
        ${renderConnectionPanel(devices, serialPorts, isProbing)}
        ${state.activeTab === "latency" ? renderLatencyForm() : renderPollForm()}
      </aside>

      <section class="main">
        ${renderDeviceTable(devices, isProbing)}
        ${state.activeTab === "latency" ? renderLatencyMain() : renderPollMain()}
      </section>
    </section>`;

  bindUi();
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
        </label>` : ""}
    </div>`;
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
      <label><span>${t("sampleCount")}</span><input name="sampleCount" type="number" min="1" max="10000" value="${html(measure.sampleCount)}" /></label>
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
      render();
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
  document.querySelector("#autoAxisThresholdBtn")?.addEventListener("click", autoAxisThreshold);

  const pollForm = document.querySelector("#pollForm");
  pollForm?.addEventListener("submit", startPollTest);
  pollForm?.addEventListener("input", (event) => {
    syncPollForm(event.currentTarget);
  });
  pollForm?.addEventListener("change", (event) => {
    syncPollForm(event.currentTarget);
  });
  document.querySelector("#stopPollBtn")?.addEventListener("click", stopPollTest);
}

async function probe() {
  if (state.probeRunning) return;
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
    || state.pollRunning;
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
    await invoke("stop_measurement");
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

async function runDelayedPollTest(config) {
  state.pollStartDelayTimer = null;
  state.pollStartPending = false;
  state.pollStartInFlight = true;
  updateBusyControls();
  try {
    await invoke("start_poll_test", { config });
    state.pollRunning = true;
    appendLog("polling rate test started");
    startPollStatusPolling();
    if (state.pollStopAfterStart) {
      try {
        await invoke("stop_poll_test");
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
    await invoke("stop_poll_test");
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
  try {
    const status = await invoke("measurement_status");
    updateStatus(status);
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
  try {
    const status = await invoke("poll_test_status");
    updatePollStatus(status);
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

function logTimestamp() {
  const now = new Date();
  const pad = (value) => String(value).padStart(2, "0");
  return `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}

function appendLog(line) {
  const log = document.querySelector("#log");
  if (!log) return;
  const entry = `[${logTimestamp()}] ${line}`;
  log.textContent = log.textContent ? `${log.textContent}\n${entry}` : entry;
}

function setupTitlebar() {
  const appWindow = getCurrentWindow();
  document.querySelector("#win-min")?.addEventListener("click", () => appWindow.minimize());
  document.querySelector("#win-max")?.addEventListener("click", () => appWindow.toggleMaximize());
  document.querySelector("#win-close")?.addEventListener("click", () => appWindow.close());
}

setupTitlebar();
applyAppVersion();
render();
probe();
