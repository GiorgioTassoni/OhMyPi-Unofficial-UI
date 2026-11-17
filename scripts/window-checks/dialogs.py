from pathlib import Path

from window import PRELUDE, checked


SETUP = r"""
window.__askChecks = {
  thread: "01a0shell0001",
  request(id, questions) {
    return {
      id, kind: "ask", title: null, message: null, options: [], optionDetails: [],
      questions, prefill: null, placeholder: null, timeoutMs: null,
    };
  },
  questions: [
    {
      id: "database", question: "Which database?", header: "Storage", multi: false,
      recommended: 0,
      options: [
        { label: " SQLite ", description: "An embedded database" },
        { label: "Postgres", preview: "A separate database server" },
      ],
    },
    {
      id: "features", question: "Which features?", multi: true,
      options: [{ label: "Search" }, { label: "Export" }],
    },
  ],
  async present(request) {
    window.__world.approvals[this.thread] = [structuredClone(request)];
    window.__fire("session-ui-requests", {
      thread: this.thread, payload: [structuredClone(request)],
    });
    await window.__helpers.sleep(100);
  },
  async withdraw() {
    window.__world.approvals[this.thread] = [];
    window.__fire("session-ui-requests", { thread: this.thread, payload: [] });
    await window.__helpers.sleep(100);
  },
  text(element) {
    return element.textContent.replace(/\s+/g, " ").trim();
  },
  field(question) {
    const field = Array.from(document.querySelectorAll("[data-dialog-panel] fieldset"))
      .find((element) => this.text(element.querySelector("legend")).includes(question));
    if (!field) throw new Error(`Missing fieldset: ${question}`);
    return field;
  },
  legend(question) {
    return this.field(question).querySelector("legend");
  },
  option(question, label) {
    const input = Array.from(this.field(question).querySelectorAll("input"))
      .find((element) => element.value === label && element.type !== "text");
    if (!input) throw new Error(`Missing option: ${label}`);
    return input;
  },
  async type(question, value) {
    const input = this.field(question).querySelector('input[type="text"]');
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await window.__helpers.sleep(20);
  },
  button(label) {
    const button = Array.from(document.querySelectorAll("[data-dialog-panel] button"))
      .find((element) => this.text(element).toLowerCase() === label);
    if (!button) throw new Error(`Missing button: ${label}`);
    return button;
  },
  has(label) {
    return Array.from(document.querySelectorAll("[data-dialog-panel] button"))
      .some((element) => this.text(element).toLowerCase() === label);
  },
  pager(label) {
    const aria = label === "previous" ? "Previous question" : "Next question";
    const button = document.querySelector(`[data-dialog-panel] button[aria-label='${aria}']`);
    if (!button) throw new Error(`Missing pager: ${label}`);
    return button;
  },
  hasPager() {
    return document.querySelector('[data-dialog-panel] button[aria-label="Previous question"]') !== null
      || document.querySelector('[data-dialog-panel] button[aria-label="Next question"]') !== null;
  },
  collapse() {
    const button = document.querySelector(
      '[data-dialog-panel] section header button[aria-expanded="true"]',
    );
    if (!button) throw new Error("Missing collapse control");
    return button;
  },
  lastInput(question) {
    const inputs = Array.from(this.field(question).querySelectorAll("input"))
      .filter((element) => element.type !== "hidden");
    if (inputs.length === 0) throw new Error(`Missing input: ${question}`);
    return inputs[inputs.length - 1];
  },
  gapAbove(question, button) {
    return Math.round(
      button.getBoundingClientRect().top - this.lastInput(question).getBoundingClientRect().bottom,
    );
  },
  actionsShareRow(primary) {
    const dismiss = this.button("dismiss").getBoundingClientRect();
    const action = primary.getBoundingClientRect();
    return dismiss.width > 0 && dismiss.right < action.left
      && Math.abs((dismiss.top + dismiss.bottom - action.top - action.bottom) / 2) <= 1;
  },
  phase() {
    const live = document.querySelector('[data-dialog-panel] [aria-live="polite"]');
    return live ? this.text(live) : null;
  },
  submit() {
    return document.querySelector('[data-dialog-panel] button[type="submit"]');
  },
  answers(id) {
    return window.__helpers.calls("respond_ui_request")
      .filter((call) => call.args.requestId === id);
  },
};
"""


def run(harness):
    report = {}
    harness.js(PRELUDE)
    checked(report, "fixtureConversationIsReady", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const project = await h.waitFor(() => h.all('aside [role="button"]')
    .find((element) => element.textContent.includes("omp-shell-app")), 4000);
  if (!h.row("Wire the sidebar") && project) {
    project.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, button: 0 }));
    await h.sleep(100);
  }
  if (!await h.claim("Wire the sidebar", "01a0shell0001")) {
    throw new Error("Could not open the fixture conversation");
  }
  h.button("Got it")?.click();
  await h.sleep(50);
  window.__step = JSON.stringify({ ok: true });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    harness.js(SETUP)

    checked(report, "oneQuestionAtATimeKeepsDraftsAcrossPhases", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const request = a.request("ask-phases", a.questions);
  await a.present(request);
  const singleQuestion = document.querySelectorAll("[data-dialog-panel] fieldset").length === 1;
  const firstPhase = a.phase() === "1 of 2";
  const previousDisabled = a.pager("previous").disabled;
  const nextBlocked = a.button("next").disabled && a.pager("next").disabled;
  const pagerControls = a.pager("previous").type === "button"
    && a.pager("next").type === "button"
    && !a.pager("previous").parentElement.closest("button")
    && !a.pager("next").parentElement.closest("button");
  const noEarlySubmit = !a.has("submit");
  const radio = a.option("Which database?", " SQLite ");
  const nativeTypes = radio.type === "radio"
    && !a.field("Which database?").querySelector('input[type="checkbox"]');
  radio.focus();
  const radioFocusable = document.activeElement === radio && radio.tabIndex === 0
    && !radio.disabled;
  const form = document.querySelector("[data-dialog-panel] form");
  form.requestSubmit();
  a.field("Which database?").querySelector('input[type="text"]').dispatchEvent(
    new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
  );
  await h.sleep(50);
  const invalidStays = a.phase() === "1 of 2"
    && a.answers(request.id).length === 0;
  radio.click();
  await h.waitFor(() => !a.button("next").disabled, 1000);
  const nextEnabled = !a.button("next").disabled && !a.pager("next").disabled;
  a.pager("next").click();
  await h.sleep(50);
  const advanced = a.phase() === "2 of 2" && !a.has("next") && a.has("submit")
    && a.pager("next").disabled;
  const legendFocused = document.activeElement === a.legend("Which features?");
  const noEarlyAnswer = a.answers(request.id).length === 0;
  await h.waitFor(() => !a.pager("previous").disabled, 1000);
  a.pager("previous").click();
  await h.sleep(50);
  const backRetains = a.phase() === "1 of 2"
    && a.option("Which database?", " SQLite ").checked && a.pager("previous").disabled;
  const backFocused = document.activeElement === a.legend("Which database?");
  const backRadio = a.option("Which database?", " SQLite ");
  backRadio.focus();
  const backRadioFocusable = backRadio.checked && document.activeElement === backRadio;
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  const nextFocused = document.activeElement === a.legend("Which features?");
  const checkbox = a.option("Which features?", "Search");
  checkbox.click();
  checkbox.focus();
  await h.sleep(30);
  await a.present(structuredClone(request));
  const refreshed = a.phase() === "2 of 2"
    && a.option("Which features?", "Search").checked
    && document.activeElement === a.option("Which features?", "Search");
  window.__step = JSON.stringify({
    ok: singleQuestion && firstPhase && previousDisabled && nextBlocked && pagerControls
      && noEarlySubmit && nativeTypes && radioFocusable && invalidStays && nextEnabled
      && advanced && legendFocused && noEarlyAnswer && backRetains && backFocused
      && backRadioFocusable && nextFocused && refreshed,
    singleQuestion, firstPhase, previousDisabled, nextBlocked, pagerControls, noEarlySubmit,
    nativeTypes, radioFocusable, invalidStays, nextEnabled, advanced, legendFocused,
    noEarlyAnswer, backRetains, backFocused, backRadioFocusable, nextFocused, refreshed,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    snapshot = Path(__file__).resolve().parents[2] / ".research" / "checks" / "native-ask.png"
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    harness.snapshot(str(snapshot))

    checked(report, "multiselectCheckboxesStayIndependentAcrossPhases", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-multi", a.questions));
  a.option("Which database?", "Postgres").click();
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  const search = a.option("Which features?", "Search");
  const exportOption = a.option("Which features?", "Export");
  const checkboxTypes = search.type === "checkbox" && exportOption.type === "checkbox";
  search.click();
  exportOption.click();
  await h.sleep(30);
  const bothSelected = search.checked && exportOption.checked;
  search.click();
  await h.sleep(20);
  const independent = !search.checked && exportOption.checked;
  search.click();
  await h.sleep(20);
  await a.type("Which features?", "  Reports  ");
  await h.waitFor(() => !a.pager("previous").disabled, 1000);
  a.pager("previous").click();
  await h.sleep(50);
  const earlierKept = a.option("Which database?", "Postgres").checked;
  await h.waitFor(() => !a.pager("next").disabled, 1000);
  a.pager("next").click();
  await h.sleep(50);
  const laterKept = a.option("Which features?", "Export").checked
    && a.option("Which features?", "Search").checked
    && a.field("Which features?").querySelector('input[type="text"]').value === "  Reports  ";
  const noEarlyAnswer = a.answers("ask-multi").length === 0;
  window.__step = JSON.stringify({
    ok: checkboxTypes && bothSelected && independent && earlierKept && laterKept && noEarlyAnswer,
    checkboxTypes, bothSelected, independent, earlierKept, laterKept, noEarlyAnswer,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "oneSubmitSendsTheWholeOrderedBatchWithExactLabels", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await h.waitFor(() => !a.submit().disabled, 1000);
  const submit = a.submit();
  submit.click();
  submit.click();
  await h.sleep(150);
  const calls = a.answers("ask-multi");
  const expected = [
    { id: "database", selectedOptions: ["Postgres"] },
    { id: "features", selectedOptions: ["Export", "Search"], customInput: "Reports" },
  ];
  const answer = calls[0]?.args.answer;
  const exactAnswer = JSON.stringify(answer?.answers) === JSON.stringify(expected);
  const dismissed = !document.querySelector("[data-dialog-panel] fieldset");
  window.__step = JSON.stringify({
    ok: calls.length === 1 && exactAnswer && dismissed,
    calls: calls.length, answer, exactAnswer, dismissed,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "collapsingAnAskPreservesEveryDraftAnswer", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-collapse", a.questions));
  a.option("Which database?", "Postgres").click();
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  a.option("Which features?", "Export").click();
  await a.type("Which features?", "Keep this draft");
  const panel = document.querySelector("[data-dialog-panel]");
  a.collapse().click();
  await h.sleep(50);
  const collapsed = Array.from(panel.children)
    .find((element) => element.tagName === "BUTTON" && element.offsetParent !== null);
  if (!collapsed) throw new Error("The collapsed question summary is missing");
  collapsed.click();
  await h.sleep(50);
  const preserved = a.phase() === "2 of 2"
    && a.option("Which features?", "Export").checked
    && a.field("Which features?").querySelector('input[type="text"]').value === "Keep this draft";
  await h.waitFor(() => !a.pager("previous").disabled, 1000);
  a.pager("previous").click();
  await h.sleep(50);
  const earlierPreserved = a.option("Which database?", "Postgres").checked;
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  const noEarlyAnswer = a.answers("ask-collapse").length === 0;
  await h.waitFor(() => !a.submit().disabled, 1000);
  a.submit().click();
  await h.sleep(100);
  const answer = a.answers("ask-collapse")[0]?.args.answer;
  const exactAnswer = JSON.stringify(answer?.answers) === JSON.stringify([
    { id: "database", selectedOptions: ["Postgres"] },
    { id: "features", selectedOptions: ["Export"], customInput: "Keep this draft" },
  ]);
  window.__step = JSON.stringify({
    ok: preserved && earlierPreserved && noEarlyAnswer && exactAnswer,
    preserved, earlierPreserved, noEarlyAnswer, exactAnswer,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "singleQuestionSubmitsDirectlyWithoutPhaseControls", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-single", [a.questions[0]]));
  const noPhase = a.phase() === null;
  const noNavigation = !a.has("previous") && !a.has("next") && !a.hasPager();
  const directSubmit = a.submit() !== null && a.submit().disabled;
  await a.type("Which database?", "   ");
  const blankStillBlocked = a.submit() !== null && a.submit().disabled;
  document.querySelector("[data-dialog-panel] form").requestSubmit();
  await h.sleep(50);
  const invalidIgnored = a.submit() !== null && a.submit().disabled
    && a.answers("ask-single").length === 0
    && !a.option("Which database?", "Postgres").checked;
  a.option("Which database?", "Postgres").click();
  await h.waitFor(() => !a.submit().disabled, 1000);
  const enabled = !a.submit().disabled;
  a.submit().click();
  await h.sleep(100);
  const calls = a.answers("ask-single");
  const exactAnswer = JSON.stringify(calls[0]?.args.answer.answers) === JSON.stringify([
    { id: "database", selectedOptions: ["Postgres"] },
  ]);
  const dismissed = !document.querySelector("[data-dialog-panel] fieldset");
  window.__step = JSON.stringify({
    ok: noPhase && noNavigation && directSubmit && blankStillBlocked && invalidIgnored
      && enabled && calls.length === 1 && exactAnswer && dismissed,
    noPhase, noNavigation, directSubmit, blankStillBlocked, invalidIgnored, enabled,
    calls: calls.length, exactAnswer, dismissed,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "singleCustomAnswerAndNewRequestDoNotLeakSelections", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-custom", a.questions));
  const reset = !a.option("Which database?", " SQLite ").checked
    && !a.option("Which database?", "Postgres").checked
    && a.button("next").disabled && a.pager("previous").disabled
    && a.phase() === "1 of 2";
  a.option("Which database?", "Postgres").click();
  await h.sleep(20);
  await a.type("Which database?", "  MariaDB  ");
  const customClearsChoice = !a.option("Which database?", "Postgres").checked;
  a.option("Which database?", " SQLite ").click();
  await h.sleep(20);
  const choiceClearsCustom = a.field("Which database?").querySelector('input[type="text"]').value === "";
  await a.type("Which database?", "  MariaDB  ");
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  const emptyMultiIsReady = a.phase() === "2 of 2" && !a.submit().disabled;
  await h.waitFor(() => !a.submit().disabled, 1000);
  a.submit().click();
  await h.sleep(100);
  const answer = a.answers("ask-custom")[0]?.args.answer;
  const expected = [
    { id: "database", selectedOptions: [], customInput: "MariaDB" },
    { id: "features", selectedOptions: [] },
  ];
  const exactAnswer = JSON.stringify(answer?.answers) === JSON.stringify(expected);
  window.__step = JSON.stringify({
    ok: reset && customClearsChoice && choiceClearsCustom && emptyMultiIsReady && exactAnswer,
    reset, customClearsChoice, choiceClearsCustom, emptyMultiIsReady, answer, exactAnswer,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "failedAnswerKeepsSelectionsAndCanBeRetried", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-retry", a.questions));
  a.option("Which database?", "Postgres").click();
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  await a.type("Which features?", "Keep this draft");
  window.__world.uiAnswerError = "Temporary delivery failure";
  await h.waitFor(() => !a.submit().disabled, 1000);
  a.submit().click();
  await h.sleep(150);
  const failure = document.querySelector("[data-error]");
  const visibleFailure = failure !== null
    && h.text(failure).includes("Temporary delivery failure");
  const sameQuestion = a.phase() === "2 of 2";
  const draftKept = a.field("Which features?").querySelector('input[type="text"]').value
    === "Keep this draft";
  const retryable = a.submit() !== null && !a.submit().disabled;
  failure?.querySelector("button")?.click();
  await h.sleep(30);
  const failureDismissed = !document.querySelector("[data-error]");
  await h.waitFor(() => !a.pager("previous").disabled, 1000);
  a.pager("previous").click();
  await h.sleep(50);
  const retained = a.option("Which database?", "Postgres").checked;
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  window.__world.uiAnswerError = null;
  await h.waitFor(() => !a.submit().disabled, 1000);
  a.submit().click();
  await h.sleep(100);
  const calls = a.answers("ask-retry");
  const accepted = calls.length === 2 && calls[0].error
    && calls[1].args.answer.answers[0].selectedOptions[0] === "Postgres"
    && !document.querySelector("[data-dialog-panel] fieldset");
  window.__step = JSON.stringify({
    ok: Boolean(retryable && visibleFailure && failureDismissed && sameQuestion
      && draftKept && retained && accepted),
    retryable, visibleFailure, failureDismissed, sameQuestion, draftKept, retained,
    accepted: Boolean(accepted),
  });
} catch (error) { window.__world.uiAnswerError = null; window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "cancelAndEngineWithdrawalHaveDifferentLifecycles", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  await a.present(a.request("ask-cancel", a.questions));
  const dismiss = Array.from(document.querySelectorAll("[data-dialog-panel] button"))
    .find((element) => element.textContent.trim() === "dismiss");
  dismiss.click();
  await h.sleep(100);
  const cancellation = a.answers("ask-cancel")[0]?.args.answer;
  await a.present(a.request("ask-withdraw", a.questions));
  await a.withdraw();
  const removedWithoutReply = a.answers("ask-withdraw").length === 0
    && !document.querySelector("[data-dialog-panel] fieldset");
  window.__step = JSON.stringify({
    ok: cancellation?.cancelled === true && !cancellation.answers && removedWithoutReply,
    cancellation, removedWithoutReply,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    harness.window.resize(1400, 600)
    harness.pump(0.2)
    checked(report, "singleLongQuestionKeepsNavigationVisible", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const options = Array.from({ length: 24 }, (_, index) => ({ label: ` Choice ${index + 1} ` }));
  const questions = [
    { id: "long-1", question: "First long question", multi: false, options },
    { id: "long-2", question: "Second question", multi: false, options: [{ label: "Alpha" }, { label: "Beta" }] },
    { id: "long-3", question: "Third question", multi: false, options: [{ label: "One" }, { label: "Two" }] },
  ];
  await a.present(a.request("ask-long", questions));
  const oneAtATime = document.querySelectorAll("[data-dialog-panel] fieldset").length === 1;
  const field = a.field("First long question");
  const scroller = (() => {
    let node = field.parentElement;
    while (node && node !== document.body) {
      const overflow = getComputedStyle(node).overflowY;
      if (overflow === "auto" || overflow === "scroll") return node;
      node = node.parentElement;
    }
    return null;
  })();
  if (!scroller) throw new Error("The long question has no bounded scroll container");
  const bounded = scroller.scrollHeight > scroller.clientHeight;
  const nextBox = a.button("next").getBoundingClientRect();
  const previousBox = a.pager("previous").getBoundingClientRect();
  const nextArrowBox = a.pager("next").getBoundingClientRect();
  const controlsVisible = nextBox.top >= 0 && nextBox.bottom <= innerHeight
    && previousBox.top >= 0 && previousBox.bottom <= innerHeight
    && nextArrowBox.top >= 0 && nextArrowBox.bottom <= innerHeight
    && a.pager("previous").disabled && a.pager("next").disabled
    && !a.has("submit") && a.phase() === "1 of 3";
  scroller.scrollTop = scroller.scrollHeight;
  await h.sleep(30);
  const bounds = scroller.getBoundingClientRect();
  const lastOption = a.option("First long question", " Choice 24 ").closest("label").getBoundingClientRect();
  const lastReachable = lastOption.top >= bounds.top - 1 && lastOption.bottom <= bounds.bottom + 1;
  a.option("First long question", " Choice 24 ").click();
  await h.waitFor(() => !a.pager("next").disabled, 1000);
  a.pager("next").click();
  await h.sleep(50);
  const secondPhase = a.phase() === "2 of 3" && !a.pager("previous").disabled
    && a.pager("next").disabled;
  const resetScroll = scroller.scrollTop === 0;
  a.option("Second question", "Alpha").click();
  await h.waitFor(() => !a.button("next").disabled, 1000);
  a.button("next").click();
  await h.sleep(50);
  const submitBox = a.submit().getBoundingClientRect();
  const submitVisible = a.phase() === "3 of 3"
    && submitBox.top >= 0 && submitBox.bottom <= innerHeight
    && a.pager("next").disabled;
  a.option("Third question", "One").click();
  await h.waitFor(() => !a.submit().disabled, 1000);
  a.submit().click();
  await h.sleep(100);
  const answers = a.answers("ask-long")[0]?.args.answer.answers;
  const allAnswered = JSON.stringify(answers) === JSON.stringify([
    { id: "long-1", selectedOptions: [" Choice 24 "] },
    { id: "long-2", selectedOptions: ["Alpha"] },
    { id: "long-3", selectedOptions: ["One"] },
  ]);
  window.__step = JSON.stringify({
    ok: oneAtATime && bounded && controlsVisible && lastReachable && secondPhase
      && resetScroll && submitVisible && allAnswered,
    oneAtATime, bounded, controlsVisible, lastReachable, secondPhase, resetScroll,
    submitVisible, allAnswered,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    harness.window.resize(1400, 900)
    harness.pump(0.2)

    checked(report, "nativeColorQuestionKeepsFooterCloseToItsInput", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const color = {
    id: "accent",
    question: "Prova scelta singola: quale colore scegli? Prova anche a cambiare scelta: deve rimanerne selezionata una sola.",
    header: "Scelta singola",
    multi: false,
    options: [{ label: "Blu" }, { label: "Verde" }, { label: "Rosso" }],
  };
  const questions = [
    color,
    { id: "density", question: "How dense should the layout be?", multi: false,
      options: [{ label: "Comfortable" }, { label: "Compact" }] },
    { id: "radius", question: "How rounded should the corners be?", multi: false,
      options: [{ label: "Soft" }, { label: "Sharp" }] },
  ];
  await a.present(a.request("ask-compact", questions));
  const field = a.field(color.question);
  const optionCount = field.querySelectorAll('input[type="radio"]').length === 3;
  const blankTail = a.gapAbove(color.question, a.button("next"));
  const inlineActions = a.actionsShareRow(a.button("next"));
  const compact = blankTail >= 0 && blankTail <= 24;
  const previousBox = a.pager("previous").getBoundingClientRect();
  const nextBox = a.pager("next").getBoundingClientRect();
  const header = a.collapse().closest("header");
  const pagerInHeader = header !== null
    && a.pager("previous").closest("header") === header
    && a.pager("next").closest("header") === header
    && document.querySelector('[data-dialog-panel] [aria-live="polite"]').closest("header")
      === header;
  const phaseNode = document.querySelector('[data-dialog-panel] [aria-live="polite"]');
  const phaseIndex = (element) => Array.prototype.indexOf.call(
    header.querySelectorAll("button, [aria-live='polite']"), element,
  );
  const pagerOrder = header !== null
    && phaseIndex(a.pager("previous")) >= 0
    && phaseIndex(a.pager("previous")) < phaseIndex(phaseNode)
    && phaseIndex(phaseNode) < phaseIndex(a.pager("next"));
  const pagerVisible = a.phase() === "1 of 3"
    && a.pager("previous").disabled && a.pager("next").disabled
    && previousBox.top >= 0 && previousBox.bottom <= innerHeight
    && nextBox.top >= 0 && nextBox.bottom <= innerHeight;
  window.__step = JSON.stringify({
    ok: optionCount && compact && inlineActions && pagerInHeader && pagerOrder && pagerVisible,
    optionCount, compact, inlineActions, pagerInHeader, pagerOrder, pagerVisible, blankTail,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "singleQuestionWithDescriptionsFitsWithoutPager", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const question = {
    id: "palette",
    question: "Which palette fits the graphite theme?",
    multi: false,
    options: [
      { label: "Graphite", description: "Neutral dark surfaces with amber accents" },
      { label: "Slate", description: "Cooler dark surfaces with a muted accent" },
      { label: "Ink", description: "Deepest contrast with a brighter accent" },
    ],
  };
  await a.present(a.request("ask-single-descriptions", [question]));
  const field = a.field(question.question);
  const noPager = a.phase() === null && !a.hasPager()
    && !a.has("previous") && !a.has("next");
  const optionCount = field.querySelectorAll('input[type="radio"]').length === 3;
  const descriptionsVisible = question.options
    .every((option) => a.text(field).includes(option.description));
  const blankTail = a.gapAbove(question.question, a.submit());
  const inlineActions = a.actionsShareRow(a.submit());
  const compact = blankTail >= 0 && blankTail <= 24;
  const submitBox = a.submit().getBoundingClientRect();
  const submitVisible = submitBox.top >= 0 && submitBox.bottom <= innerHeight
    && a.submit().disabled;
  window.__step = JSON.stringify({
    ok: noPager && optionCount && descriptionsVisible && compact && inlineActions && submitVisible,
    noPager, optionCount, descriptionsVisible, compact, inlineActions, submitVisible, blankTail,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "ordinarySelectStillSubmitsItsRawLabel", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const request = a.request("ordinary-select", []);
  request.kind = "select";
  request.title = "Ordinary single choice";
  request.options = ["  Keep this exact label  ", "Another option"];
  await a.present(request);
  const option = Array.from(document.querySelectorAll("[data-dialog-panel] button"))
    .find((element) => element.textContent.includes("Keep this exact label"));
  option.click();
  await h.sleep(100);
  const answer = a.answers(request.id)[0]?.args.answer;
  window.__step = JSON.stringify({
    ok: answer?.value === "  Keep this exact label  " && !answer.answers,
    answer,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    checked(report, "ordinaryConfirmRemainsASeparateDialog", harness, r"""
(async () => { try {
  const h = window.__helpers, a = window.__askChecks;
  const request = a.request("ordinary-confirm", []);
  request.kind = "confirm";
  request.title = "Proceed with this operation?";
  await a.present(request);
  const panel = document.querySelector("[data-dialog-panel]");
  const notAnAsk = !panel.querySelector("fieldset") && !a.submit();
  const no = Array.from(panel.querySelectorAll("button"))
    .find((element) => element.textContent.trim() === "no");
  no.click();
  await h.sleep(100);
  const answer = a.answers(request.id)[0]?.args.answer;
  window.__step = JSON.stringify({
    ok: notAnAsk && answer?.confirmed === false && !answer.answers,
    notAnAsk, answer,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    return report
