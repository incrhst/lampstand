// What Setup › This computer says. Two switches, both off until somebody
// turns them on, because they are the only things Lampstand asks of the
// computer itself (docs/PRD.md, "The main machine may be someone's own
// laptop"). Kept free of Svelte so node can read it.

export const words = {
  tab: "This computer",
  tabSubtitle: "Start with the computer, stay awake",
  title: "This computer",
  startPanel: "When the computer starts",
  startSwitch: "Start Lampstand when this computer starts",
  startSentence:
    "Lampstand opens by itself when someone signs in to this computer, so the screens are ready without anyone starting it. Off by default. Turn it off here at any time.",
  awakePanel: "While the screens are showing",
  awakeSwitch: "Keep this computer awake while it is showing the screens",
  awakeSentence:
    "Stops the computer going to sleep, and its own screen going dark, while a screen window or an OBS picture is open. When the screens are closed it can sleep as usual. Off by default.",
};
