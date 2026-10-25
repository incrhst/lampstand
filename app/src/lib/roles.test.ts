import { test } from "node:test";
import assert from "node:assert/strict";
import type { RoleName, RoleView, TeamStatus } from "./api";
import {
  ROLES,
  allowBlocked,
  cannotGoLive,
  deviceRunsLine,
  operatorTakenBackNotice,
  othersRun,
  ownRunsLine,
  runsLine,
  streamOfflineLine,
  isStreamDesk,
  screensLine,
  shortcutsOff,
  takeoverOption,
  withRoster,
  allowLabel,
  job,
  noticeRoles,
  operatorTakenBack,
  replacedNotice,
  requestPrompt,
  roleChoice,
  runFromHere,
  takeName,
  runs,
  wantsAttention,
} from "./roles.ts";

const view = (role: RoleName, over: Partial<RoleView> = {}): RoleView => ({
  role,
  holds: false,
  standing: "Ask",
  holder: "aa",
  holder_name: "Mary's laptop",
  offline_secs: null,
  wait_secs: null,
  blocked: null,
  request: null,
  asking: null,
  replaced_by: null,
  always: [],
  holder_waits_secs: null,
  offers_always: role === "Operator" || role === "Stream",
  ...over,
});
const team = (roles: RoleView[], over: Partial<TeamStatus> = {}): TeamStatus =>
  ({ roles, paired: [], presenter: {}, scene_cue_waiting: false, ...over }) as unknown as TeamStatus;
/** A paired church: `paired` has one device, so the roster has something to say. */
const paired = (roles: RoleView[], over: Partial<TeamStatus> = {}): TeamStatus =>
  team(roles, { paired: [{ device: "aa", name: "Mary's laptop" }] as TeamStatus["paired"], ...over });
const pill = { tone: "ready" as const, text: "Synced with 1 device", title: "Shared" };
const mine = (role: RoleName) => view(role, { holds: true, standing: "Holding", holder: "me", holder_name: "Sanctuary" });

test("each role is named as the spec's roster names it", () => {
  assert.deepEqual(ROLES.map(runs), ["Runs the screens", "Runs the service", "Runs the stream", "Runs the sound"]);
});

test("the prompt is the spec's, for every role", () => {
  assert.equal(requestPrompt("Stream", "Mary's laptop"), "Mary's laptop wants to run the stream.");
  assert.equal(requestPrompt("Operator", "Mary's laptop"), "Mary's laptop wants to run the service.");
  assert.equal(requestPrompt("Projection", "Mary's laptop"), "Mary's laptop wants to run the screens. Allowing clears the screens.");
  assert.equal(requestPrompt("Audio", "Mary's laptop"), "Mary's laptop wants to run the sound. Allowing stops the music.");
  assert.equal(requestPrompt("Stream", ""), "Another device wants to run the stream.");
});

test("only Projection and Audio say what they take off air", () => {
  assert.equal(allowLabel("Projection"), "Allow and clear the screens");
  assert.equal(allowLabel("Audio"), "Allow and stop the music");
  assert.equal(allowLabel("Operator"), "Allow");
  assert.equal(allowLabel("Stream"), "Allow");
});

test("the notice for a lost role names the role", () => {
  assert.equal(
    replacedNotice("Stream", "Mary's laptop"),
    "Mary's laptop took over running the stream while this device was offline. This device no longer runs the stream.",
  );
  assert.ok(replacedNotice("Audio", "Sam").includes("the sound"));
});

test("the Take button", () => {
  assert.deepEqual(roleChoice(null), { kind: "none" });
  assert.deepEqual(roleChoice(view("Stream", { holds: true, standing: "Holding" })), { kind: "none" });
  assert.equal(roleChoice(view("Stream", { standing: "Open", holder: null })).kind, "claim");
  assert.equal(roleChoice(view("Stream")).kind, "ask");
  const t = roleChoice(view("Stream", { standing: "TakeOver", offline_secs: 130 }));
  assert.deepEqual(t, {
    kind: "takeover",
    label: "Take",
    confirm: "Mary's laptop has been offline for 2 minutes. Run the stream from here?",
    ok: "Take",
  });
  const w = roleChoice(view("Audio", { standing: "Wait", offline_secs: 30, wait_secs: 90 }));
  assert.equal(w.kind === "wait" && w.hint, "Mary's laptop went offline 30 seconds ago. You can take over in 2 minutes.");
});

test("the wait hint says so when the holder's own setting is the longer one", () => {
  const w = roleChoice(view("Audio", { standing: "Wait", offline_secs: 30, wait_secs: 100, holder_waits_secs: 120 }));
  assert.equal(
    w.kind === "wait" && w.hint,
    "Mary's laptop went offline 30 seconds ago. You can take over in 2 minutes. Mary's laptop is set to wait 2 minutes, which is longer than this device.",
  );
});

test("a greyed Take says why", () => {
  const none = roleChoice(view("Projection", { blocked: "NoScreens" }));
  assert.equal(none.kind === "blocked" && none.hint, "No screens are set up on this device");
  const op = roleChoice(view("Operator", { blocked: "NeedsProjection", standing: "Open", holder: null }));
  assert.equal(op.kind === "blocked" && op.hint, "The service can't be run until someone runs the screens.");
});

test("an ask shows its answer, and a second asker is told who is asking", () => {
  assert.equal(roleChoice(view("Stream", { asking: "Waiting" })).kind, "waiting");
  const no = roleChoice(view("Stream", { asking: "Declined" }));
  assert.equal(no.kind === "declined" && no.label, "Mary's laptop said not now.");
  const busy = roleChoice(view("Stream", { asking: { Busy: { by: "Sam's tablet" } } }));
  assert.equal(busy.kind === "busy" && busy.label, "Sam's tablet is already asking.");
});

test("a church with one device has nothing to say", () => {
  assert.equal(wantsAttention(team([])), false);
  assert.equal(wantsAttention(null), false);
  const quiet = team(ROLES.map((r) => view(r, { holds: true, standing: "Holding", holder: "me", holder_name: "Sanctuary" })));
  assert.equal(wantsAttention(quiet), false);
  assert.deepEqual(noticeRoles(quiet), []);
  // No button for a role this device holds.
  for (const r of quiet.roles) assert.deepEqual(roleChoice(r), { kind: "none" });
});

test("the Operator notice stays quiet when the screens' device holds the seat (compared by device key)", () => {
  const t = team([
    view("Projection", { holder: "sanct", holder_name: "Sanctuary" }),
    view("Operator", { holder: "sanct", replaced_by: "Sanctuary" }),
  ]);
  assert.equal(operatorTakenBack(t), true);
  assert.deepEqual(noticeRoles(t), []);
  // Another device with the same name is not the screens' device.
  const other = team([
    view("Projection", { holder: "sanct", holder_name: "Sanctuary" }),
    view("Operator", { holder: "sam", holder_name: "Sanctuary", replaced_by: "Sanctuary" }),
  ]);
  assert.equal(operatorTakenBack(other), false);
  assert.equal(noticeRoles(other).length, 1);
  assert.equal(job("Operator"), "the service");
});

test("an unanswered ask whose holder then goes offline shows the way to take over", () => {
  const waiting = view("Stream", { asking: "Waiting", standing: "Ask" });
  assert.equal(roleChoice(waiting).kind, "waiting");
  const wait = roleChoice({ ...waiting, standing: "Wait", offline_secs: 10, wait_secs: 110 });
  assert.equal(wait.kind, "wait");
  const over = roleChoice({ ...waiting, standing: "TakeOver", offline_secs: 130 });
  assert.equal(over.kind, "takeover");
  assert.equal(wantsAttention(team([{ ...waiting, standing: "TakeOver", offline_secs: 130 }])), false);
  assert.equal(wantsAttention(team([waiting])), true);
  // An answer that came back stays.
  assert.equal(roleChoice({ ...waiting, asking: "Declined", standing: "TakeOver" }).kind, "declined");
});

test("Allow is not offered for the Operator while nobody runs the screens", () => {
  const none = team([view("Projection", { holder: null, holder_name: "", standing: "Open" }), view("Operator", { holds: true })]);
  assert.equal(allowBlocked(none, "Operator"), "The service can't be run until someone runs the screens.");
  assert.equal(allowBlocked(none, "Stream"), null);
  const held = team([view("Projection"), view("Operator", { holds: true })]);
  assert.equal(allowBlocked(held, "Operator"), null);
});

test("a button standing alone names what it does", () => {
  assert.equal(runFromHere("Operator"), "Run the service from here");
  assert.equal(takeName("Operator", runFromHere("Operator")), "Run the service from here");
  assert.equal(takeName("Stream"), "Take the stream");
});

test("a device says what it runs, in the spec's verbs, or that it is planning", () => {
  assert.equal(runsLine([]), "Planning");
  assert.equal(runsLine(["Projection"]), "Runs the screens");
  assert.equal(runsLine(["Projection", "Operator"]), "Runs the screens and the service");
  assert.equal(runsLine(["Projection", "Operator", "Audio"]), "Runs the screens, the service and the sound");
  assert.equal(runsLine(["Stream"]), "Runs the stream");
});

test("the status bar names what this device runs and who runs the rest", () => {
  const t = paired([mine("Projection"), mine("Operator"), view("Stream"), mine("Audio")]);
  assert.equal(ownRunsLine(t), "Runs the screens, the service and the sound");
  assert.deepEqual(othersRun(t), ["Mary's laptop runs the stream"]);
  const shown = withRoster(pill, t);
  assert.equal(shown.text, "Runs the screens, the service and the sound · Mary's laptop runs the stream · Synced with 1 device");
  assert.match(shown.title, /Shared\. Runs the screens/);
  // Nothing in it is the 0.7.0 wording.
  assert.doesNotMatch(shown.text + shown.title, /Presenting|Driving|presenter|drive/);
});

test("a device that runs nothing is Planning, and the others are named once each", () => {
  const t = paired([
    view("Projection", { holder: "aa", holder_name: "Sanctuary PC" }),
    view("Operator", { holder: "aa", holder_name: "Sanctuary PC" }),
    view("Stream", { holder: "bb", holder_name: "Sam's PC" }),
    view("Audio", { holder: null, holder_name: "", standing: "Open" }),
  ]);
  assert.equal(ownRunsLine(t), "Planning");
  assert.deepEqual(othersRun(t), ["Sanctuary PC runs the screens and the service", "Sam's PC runs the stream"]);
  assert.equal(
    withRoster(pill, t).text,
    "Planning · Sanctuary PC runs the screens and the service · Sam's PC runs the stream · Synced with 1 device",
  );
  // Sync trouble keeps its tone and its words.
  const warn = { tone: "warn" as const, text: "Offline: 1 device not reachable", title: "x" };
  assert.equal(withRoster(warn, t).tone, "warn");
  assert.match(withRoster(warn, t).text, /Offline: 1 device not reachable$/);
});

test("Setup > Devices says what each paired device runs", () => {
  const t = paired([
    view("Projection", { holder: "aa", holder_name: "Sanctuary PC" }),
    mine("Operator"),
    view("Stream", { holder: "aa", holder_name: "Sanctuary PC" }),
    view("Audio", { holder: "bb" }),
  ]);
  assert.equal(deviceRunsLine(t, "aa"), "Runs the screens and the stream");
  assert.equal(deviceRunsLine(t, "bb"), "Runs the sound");
  assert.equal(deviceRunsLine(t, "cc"), "Planning");
  assert.equal(ownRunsLine(t), "Runs the service");
});

test("a church with one device sees no roster in the status bar", () => {
  const lone = team(ROLES.map(mine));
  assert.deepEqual(withRoster({ tone: "neutral" as const, text: "This device only", title: "Nothing is paired" }, lone), {
    tone: "neutral",
    text: "This device only",
    title: "Nothing is paired",
  });
  assert.equal(withRoster(pill, null), pill);
  assert.equal(withRoster(pill, team([])), pill);
  assert.equal(streamOfflineLine(lone), null);
  assert.equal(wantsAttention(lone), false);
});

test("the stream line is the owner's, only when the cue is waiting and the holder is offline", () => {
  const offline = view("Stream", { standing: "Wait", offline_secs: 20, wait_secs: 100 });
  const wording = "Mary's laptop runs the stream and is offline. OBS will catch up when it is back.";
  assert.equal(streamOfflineLine(paired([offline], { scene_cue_waiting: true })), wording);
  assert.equal(streamOfflineLine(paired([{ ...offline, standing: "TakeOver" }], { scene_cue_waiting: true })), wording);
  // The field is false until the OBS cue work sets it: no line.
  assert.equal(streamOfflineLine(paired([offline])), null);
  // Connected holder, nobody holding, or this device holding: no line.
  assert.equal(streamOfflineLine(paired([view("Stream")], { scene_cue_waiting: true })), null);
  assert.equal(streamOfflineLine(paired([view("Stream", { holder: null, standing: "Open" })], { scene_cue_waiting: true })), null);
  assert.equal(streamOfflineLine(paired([mine("Stream")], { scene_cue_waiting: true })), null);
  assert.equal(
    streamOfflineLine(paired([{ ...offline, holder_name: "" }], { scene_cue_waiting: true })),
    "Another device runs the stream and is offline. OBS will catch up when it is back.",
  );
});

test("the takeover time and the other Devices words", () => {
  assert.equal(takeoverOption(120), "2 minutes offline");
  assert.equal(takeoverOption(30), "30 seconds offline");
  assert.equal(
    cannotGoLive("Sanctuary PC"),
    "This device does not run the screens. Sanctuary PC runs the screens. To take a service live here, run the screens from here, or run the service from the bar above.",
  );
  assert.match(cannotGoLive(""), /Nobody runs the screens/);
  assert.equal(operatorTakenBackNotice("Sanctuary PC"), "Sanctuary PC took back running the service. This device no longer runs the service.");
});

test("only a paired device that runs the stream and not the service is a stream desk", () => {
  const deskRoles = [view("Projection"), view("Operator"), mine("Stream"), view("Audio")];
  assert.equal(isStreamDesk(paired(deskRoles)), true);
  // One device: nothing is paired, so no new view whatever the roles say.
  assert.equal(isStreamDesk(team([mine("Projection"), mine("Operator"), mine("Stream"), mine("Audio")])), false);
  assert.equal(isStreamDesk(team(deskRoles)), false);
  // Stream and the service together: Present as it always was.
  assert.equal(isStreamDesk(paired([view("Projection"), mine("Operator"), mine("Stream"), view("Audio")])), false);
  // Does not run the stream.
  assert.equal(isStreamDesk(paired([mine("Projection"), mine("Operator"), view("Stream"), mine("Audio")])), false);
  assert.equal(isStreamDesk(null), false);
});

test("the stream desk says who runs the screens, in the existing words", () => {
  const roles = [view("Projection", { holder_name: "Sanctuary" }), mine("Stream")];
  assert.equal(screensLine(paired(roles)), "Sanctuary runs the screens.");
  assert.equal(screensLine(paired([mine("Projection"), mine("Stream")])), null);
  assert.equal(screensLine(paired([view("Projection", { holder: null, holder_name: "" })])), null);
});

test("the stream desk says when the device that runs the screens is offline", () => {
  const off = (over: Partial<RoleView>) => paired([view("Projection", { holder_name: "Sanctuary", ...over }), mine("Stream")]);
  const line = "Sanctuary runs the screens and is offline. Scene changes wait for it.";
  assert.equal(screensLine(off({ standing: "Wait", offline_secs: 30, wait_secs: 90 })), line);
  assert.equal(screensLine(off({ standing: "TakeOver", offline_secs: 200 })), line);
  assert.equal(screensLine(off({ standing: "Ask" })), "Sanctuary runs the screens.");
});

test("a stream machine that also runs the screens keeps the whole Present", () => {
  assert.equal(isStreamDesk(paired([mine("Projection"), view("Operator"), mine("Stream"), view("Audio")])), false);
  // Nobody runs the service, or its holder is offline: still a stream desk.
  assert.equal(isStreamDesk(paired([view("Projection"), view("Operator", { holder: null, holder_name: "" }), mine("Stream")])), true);
  assert.equal(isStreamDesk(paired([view("Projection"), view("Operator", { standing: "Wait", offline_secs: 60 }), mine("Stream")])), true);
  assert.equal(isStreamDesk(paired([])), false);
});

test("shortcuts are off on a stream desk without a service, and inside the docked panel", () => {
  assert.equal(shortcutsOff(true, false, false), true);
  assert.equal(shortcutsOff(false, false, true), true);
  assert.equal(shortcutsOff(false, false, false), false);
  assert.equal(shortcutsOff(true, true, false), false);
});
