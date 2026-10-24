import { test } from "node:test";
import assert from "node:assert/strict";
import type { RoleName, RoleView, TeamStatus } from "./api";
import {
  ROLES,
  allowLabel,
  job,
  noticeRoles,
  operatorTakenBack,
  replacedNotice,
  requestPrompt,
  roleChoice,
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
  offers_always: role === "Operator" || role === "Stream",
  ...over,
});
const team = (roles: RoleView[]): TeamStatus =>
  ({ roles, paired: [], presenter: {} }) as unknown as TeamStatus;

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

test("a greyed Take says why", () => {
  const none = roleChoice(view("Projection", { blocked: "NoScreens" }));
  assert.equal(none.kind === "blocked" && none.hint, "No screens are set up on this device");
  const op = roleChoice(view("Operator", { blocked: "NeedsProjection", standing: "Open", holder: null }));
  assert.equal(op.kind === "blocked" && op.hint, "Nobody runs the screens.");
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

test("the Operator notice stays quiet when the screens' device took the seat back", () => {
  const t = team([
    view("Projection", { holder_name: "Sanctuary" }),
    view("Operator", { replaced_by: "Sanctuary" }),
  ]);
  assert.equal(operatorTakenBack(t), true);
  assert.deepEqual(noticeRoles(t), []);
  const other = team([view("Projection", { holder_name: "Sanctuary" }), view("Operator", { replaced_by: "Sam" })]);
  assert.equal(noticeRoles(other).length, 1);
  assert.equal(job("Operator"), "the service");
});
