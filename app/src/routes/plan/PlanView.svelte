<!-- Service planning: services down the left, the selected service's
     order in the middle, the song, scripture, deck and media library on the right.
     T49 replaces this with the collaborative planner. -->
<script lang="ts">
  import PresentAction from "../../lib/kit/PresentAction.svelte";
  import { untrack } from "svelte";
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import {
    request,
    type Deck,
    type ItemEstimate,
    type ItemKind,
    type MediaFile,
    type Service,
    type Sound,
    type SoundFile,
    type SoundFileStatus,
    type SoundSlot,
    type ServiceSummary,
    type SongHit,
    type Translation,
  } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import ThemePicker from "../../lib/kit/ThemePicker.svelte";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import DeckThumb from "../../lib/kit/DeckThumb.svelte";
  import MediaThumb from "../../lib/kit/MediaThumb.svelte";
  import { deckExtensions, deckSourceLabel, imageExtensions, mediaKind, onePath, videoExtensions } from "../../lib/media";
  import {
    audioExtensions,
    isAudio,
    isComing,
    isMissing,
    offerSound,
    soundSubtitle,
    soundTotalLabel,
    words as soundWords,
  } from "../../lib/sound";
  import SoundEditor from "./SoundEditor.svelte";
  import Library from "./Library.svelte";
  import ObsScenePicker from "./ObsScenePicker.svelte";
  import SongOrder from "./SongOrder.svelte";
  import TextEditor from "./TextEditor.svelte";
  import { splitText, textTitle } from "../../lib/textSlides";
  import LoopPicker from "./LoopPicker.svelte";
  import LoopPanel from "./LoopPanel.svelte";
  import { coveredAt, estimatePass, passLabel, sectionEnd, sectionOf, sectionSize, type Row } from "../../lib/loops";
  import { formatPassage } from "../../lib/passage";
  import { comingSunday, deviceZone, formatWhen, fromInputs, timeZones, toInputs } from "../../lib/when";

  let { onopened }: { onopened: () => void } = $props();

  let services: ServiceSummary[] = $state([]);
  let selected: Service | null = $state(null);
  let creating = $state(false);
  let newTitle = $state("Sunday service");
  /** The service the New service form copies its items from, or "" for an empty one. */
  let copyFrom = $state("");
  /** A date and time as the inputs hold them, in an IANA zone. */
  type When = { date: string; time: string; zone: string };
  /** The coming Sunday at 10:00 in `zone`, as the inputs hold it. */
  const defaultWhen = (zone = deviceZone()): When => ({ ...toInputs(comingSunday(Date.now(), zone), zone), zone });
  /** The New service form's date, time and zone. */
  let newWhen: When = $state(defaultWhen());
  /** The header's date, time and zone while the planner changes them. */
  let editWhen: When | null = $state(null);
  const at = (w: When) => fromInputs(w.date, w.time, w.zone);
  /** Title and author of every song we have looked up, by song ID. */
  let songInfo: Record<
    string,
    { title: string; author: string | null; sections?: string[]; order?: string[] }
  > = $state({});
  let busy = $state(false);
  /** Songs already looked up with GetSong, so each is fetched once. */
  const lookedUp = new Set<string>();

  let decks: Deck[] = $state([]);
  let deckById = $derived(Object.fromEntries(decks.map((d) => [d.id, d])) as Record<string, Deck>);

  let translations: Translation[] = $state([]);
  let translation = $state("");
  let abbreviation = $derived(
    Object.fromEntries(translations.map((t) => [t.id, t.abbreviation || t.name])) as Record<string, string>,
  );

  /** Services saved by a newer Lampstand, and files that could not be loaded. */
  let newer = $state(0);
  let damaged: string[] = $state([]);

  /** Deletes a service file that could not be loaded. */
  async function removeDamaged(id: string) {
    try {
      await request({ area: "Service", request: { Delete: id } }, "Ok");
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  async function refresh() {
    try {
      const list = await request({ area: "Service", request: "ListChecked" }, "ServiceList");
      services = list.services;
      newer = list.newer;
      damaged = list.damaged;
    } catch (e) {
      report(e);
    }
  }

  /** The row being renamed, and the row whose delete awaits a second click. */
  let renaming: string | null = $state(null);
  let renameTitle = $state("");
  let confirmDelete: string | null = $state(null);
  let showOlder = $state(false);

  /** A service that started before today is "older" and stays out of the list. */
  const startOfToday = () => new Date().setHours(0, 0, 0, 0);
  const isOlder = (s: ServiceSummary) => s.starts_at != null && s.starts_at < startOfToday();
  let olderCount = $derived(services.filter((s) => isOlder(s) && s.id !== selected?.id).length);
  let shown = $derived(
    showOlder ? services : services.filter((s) => !isOlder(s) || s.id === selected?.id),
  );

  function startRename(s: ServiceSummary) {
    confirmDelete = null;
    renaming = s.id;
    renameTitle = s.title;
  }

  async function saveRename() {
    const id = renaming;
    if (!id) return;
    renaming = null;
    if (!renameTitle.trim()) return;
    try {
      const renamed = await request(
        { area: "Service", request: { Rename: { service: id, title: renameTitle } } },
        "Service",
      );
      if (selected?.id === id) selected = renamed;
      if (live.loaded?.service === id) live.loaded = { ...live.loaded, title: renamed.title };
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  async function takeOffAir(id: string) {
    try {
      await request({ area: "Service", request: { Close: id } }, "Ok");
      if (live.loaded?.service === id) live.loaded = null;
    } catch (e) {
      report(e);
    }
  }

  async function deleteService(id: string) {
    if (confirmDelete !== id) {
      confirmDelete = id;
      return;
    }
    confirmDelete = null;
    try {
      await request({ area: "Service", request: { Delete: id } }, "Ok");
      if (live.loaded?.service === id) live.loaded = null;
      if (selected?.id === id) {
        selected = null;
        live.planning = null;
      }
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  function toggleCreate() {
    creating = !creating;
    // Each new service starts on the coming Sunday unless the planner
    // picks another day.
    copyFrom = "";
    if (creating) newWhen = defaultWhen();
  }

  /** Picking a service to start from titles the copy after it. */
  function pickCopy() {
    const source = services.find((s) => s.id === copyFrom);
    newTitle = source ? `${source.title} (copy)` : "Sunday service";
  }

  /** Offers the New service form with `id` as its starting point. */
  function startFrom(id: string) {
    const source = services.find((s) => s.id === id);
    creating = true;
    newWhen = defaultWhen();
    copyFrom = id;
    if (source) newTitle = `${source.title} (copy)`;
  }

  async function create() {
    const title = newTitle.trim() || "Service";
    try {
      selected = await request(
        {
          area: "Service",
          request: copyFrom
            ? { Duplicate: { service: copyFrom, title, starts_at: at(newWhen), time_zone: newWhen.zone } }
            : { Create: { title, starts_at: at(newWhen), time_zone: newWhen.zone } },
        },
        "Service",
      );
      live.planning = selected.id;
      creating = false;
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  async function setTheme(theme: string | null) {
    if (!selected) return;
    try {
      selected = await request({ area: "Service", request: { SetTheme: { service: selected.id, theme } } }, "Service");
    } catch (e) {
      report(e);
    }
  }

  async function reschedule(when: When | null) {
    if (!selected) return;
    const startsAt = when && at(when);
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            Reschedule: {
              service: selected.id,
              starts_at: startsAt,
              time_zone: startsAt == null ? null : when!.zone,
            },
          },
        },
        "Service",
      );
      editWhen = null;
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  const saveWhen = () => editWhen && reschedule(editWhen);
  const cancelWhen = () => (editWhen = null);

  /**
   * Enter submits and Escape cancels from any field. The kit's Button is
   * `type="button"`, so a form of several fields has no implicit submit.
   */
  function onKeys(submit: () => void, cancel?: () => void) {
    return (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        submit();
      } else if (e.key === "Escape" && cancel) {
        cancel();
      }
    };
  }

  function startReschedule() {
    if (!selected) return;
    const s = selected;
    editWhen =
      s.starts_at == null
        ? defaultWhen(s.time_zone ?? deviceZone())
        : { ...toInputs(s.starts_at, s.time_zone ?? deviceZone()), zone: s.time_zone ?? deviceZone() };
  }

  async function select(id: string) {
    editWhen = null;
    try {
      selected = await request({ area: "Service", request: { Get: id } }, "Service");
      live.planning = id;
    } catch (e) {
      report(e);
    }
  }

  function songsFound(hits: SongHit[]) {
    for (const h of hits) songInfo[h.song] ??= { title: h.title, author: null };
  }

  // Look up songs in the order that a search has not brought in, and
  // every song's author, which search hits do not carry.
  async function lookUpSongs(service: Service) {
    for (const [, item] of ordered(service)) {
      const k = item.kind;
      if (!("Song" in k)) continue;
      const id = k.Song.song;
      if (lookedUp.has(id)) continue;
      lookedUp.add(id);
      try {
        const song = await request({ area: "Library", request: { GetSong: id } }, "Song");
        songInfo[id] = {
          title: song.title,
          author: song.author,
          sections: song.sections.map((x) => x.label),
          order: song.arrangement,
        };
      } catch {
        // A song missing from the library shows as "Song".
      }
    }
  }

  /** Inserts an item at `index`, or at the end of the selected service. */
  async function addItem(kind: ItemKind, index?: number): Promise<boolean> {
    if (!selected) return false;
    const before = new Set(Object.keys(selected.items));
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            InsertItem: {
              service: selected.id,
              index: index ?? Object.keys(selected.items).length,
              kind,
            },
          },
        },
        "Service",
      );
      // A new text slide has nothing yet: open it to be typed into.
      if ("Text" in kind) openCard = Object.keys(selected.items).find((id) => !before.has(id)) ?? openCard;
      return true;
    } catch (e) {
      report(e);
      return false;
    }
  }

  /** A new, empty text slide at the end of the service. */
  function addText() {
    return addItem({ Text: { title: "", body: "", attribution: null } });
  }

  /** Rewrites a text slide item. */
  async function setText(item: string, title: string, body: string, attribution: string | null) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { SetItemText: { service: selected.id, item, title, body, attribution } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  async function loadDecks() {
    try {
      decks = await request({ area: "Library", request: "ListDecks" }, "Decks");
    } catch (e) {
      report(e);
    }
  }

  async function loadTranslations() {
    try {
      translations = await request({ area: "Library", request: "ListTranslations" }, "Translations");
      translations.sort((a, b) => a.name.localeCompare(b.name));
      if (!translations.some((t) => t.id === translation)) translation = translations[0]?.id ?? "";
    } catch (e) {
      report(e);
    }
  }

  /** Puts a newly added deck in the library list and the service. */
  async function deckAdded(deck: Deck) {
    live.status = `Added ${deck.title}`;
    await loadDecks();
    await addItem({ Deck: { deck: deck.id } });
  }

  // Parsing checks the deck, so a big one takes a moment.
  async function pickDeck() {
    try {
      const path = onePath(
        await pickFiles({
          title: "Add a PowerPoint deck",
          filters: [{ name: "PowerPoint", extensions: deckExtensions }],
        }),
      );
      if (!path) return;
      busy = true;
      await deckAdded(await request({ area: "Library", request: { AddDeckFile: { path } } }, "Deck"));
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function addDeckLink(url: string): Promise<boolean> {
    busy = true;
    try {
      await deckAdded(await request({ area: "Library", request: { AddDeckFromLink: { url } } }, "Deck"));
      return true;
    } catch (e) {
      report(e);
      return false;
    } finally {
      busy = false;
    }
  }

  // Video is copied into the library, so a long one takes a moment.
  async function pickMedia() {
    if (!selected) return;
    try {
      const path = onePath(
        await pickFiles({
          title: "Add a video or image",
          filters: [
            { name: "Video and images", extensions: [...videoExtensions, ...imageExtensions] },
            { name: "Video", extensions: videoExtensions },
            { name: "Images", extensions: imageExtensions },
          ],
        }),
      );
      if (!path) return;
      busy = true;
      live.status = "Copying into the library…";
      const file = await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media");
      await addMedia(file);
      live.status = `Added ${file.name}`;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  /** A sound as a file arrives: it waits for somebody to press play. */
  function newSound(file: SoundFile): Sound {
    return { name: file.name, file: file.file, length_ms: file.length_ms, start: "Waits", keep_off_stream: false };
  }

  /** Changes the sound of a music-only item, or a song's backing track. */
  async function setSound(item: string, slot: SoundSlot, sound: Sound | null) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { SetSound: { service: selected.id, item, slot, sound } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  /** Picks an audio file and adds it as a music-only item. */
  async function pickSound() {
    if (!selected) return;
    try {
      const path = onePath(
        await pickFiles({
          title: soundWords.pickTitle,
          filters: [{ name: soundWords.pickFilter, extensions: audioExtensions }],
        }),
      );
      if (!path) return;
      busy = true;
      live.status = "Copying into the library…";
      const file = await request({ area: "Library", request: { AddSoundFile: { path } } }, "SoundFile");
      await addItem({ Sound: { sound: newSound(file) } });
      live.status = `Added ${file.name}`;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  /** Picks an audio file and makes it the backing track of a song. */
  async function addBacking(item: string) {
    try {
      const path = onePath(
        await pickFiles({
          title: soundWords.pickTitle,
          filters: [{ name: soundWords.pickFilter, extensions: audioExtensions }],
        }),
      );
      if (!path) return;
      busy = true;
      live.status = "Copying into the library…";
      const file = await request({ area: "Library", request: { AddSoundFile: { path } } }, "SoundFile");
      await setSound(item, "Backing", newSound(file));
      live.status = `Added ${file.name}`;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  /** The song whose backing track is being asked about before it goes. */
  let confirmBacking: string | null = $state(null);

  /** Where each sound file of the open service stands on this device. */
  let soundStatus: SoundFileStatus[] = $state([]);
  let soundTicket = 0;
  async function loadSoundStatus(service: Service) {
    const ticket = ++soundTicket;
    try {
      const list = await request({ area: "Service", request: { SoundStatus: service.id } }, "SoundStatus");
      if (ticket === soundTicket) soundStatus = list;
    } catch {
      // Without it Plan cannot say a file is missing, and says nothing.
    }
  }

  async function addMedia(file: MediaFile) {
    await addItem({ Media: { blob: file.blob, name: file.name, looped: false } });
  }

  async function remove(item: string) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { RemoveItem: { service: selected.id, item } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  /** The OBS scene an item switches to when it goes live (T90). */
  async function setObsScene(item: string, scene: string | null) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { SetObsScene: { service: selected.id, item, scene } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  /** This service's order for a song item; null goes back to the song's own. */
  async function setOrder(item: string, arrangement: string[] | null) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { SetItemArrangement: { service: selected.id, item, arrangement } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  /** Loop an item, or a header's section: seconds per slide, or null for none. */
  async function setLoop(item: string, seconds: number | null) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { SetLoop: { service: selected.id, item, seconds } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  /** Makes an order the song's own, in every service, and drops this service's copy. */
  async function saveSongOrder(item: string, songId: string, order: string[]) {
    try {
      const song = await request({ area: "Library", request: { GetSong: songId } }, "Song");
      song.arrangement = order;
      await request({ area: "Library", request: { PutSong: song } }, "Song");
      songInfo[songId] = { ...songInfo[songId], order };
      await setOrder(item, null);
    } catch (e) {
      report(e);
    }
  }

  async function move(item: string, index: number) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { MoveItem: { service: selected.id, item, index } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  async function openService() {
    if (!selected) return;
    try {
      live.loaded = await request({ area: "Service", request: { Open: selected.id } }, "Loaded");
      onopened();
    } catch (e) {
      report(e);
    }
  }

  /** The version of a deck's file, so its thumbnail is drawn again when the deck changes. */
  function deckVersion(id: string): string {
    return decks.find((d) => d.id === id)?.current.blob ?? "";
  }

  function ordered(service: Service) {
    return Object.entries(service.items)
      .filter((entry): entry is [string, NonNullable<(typeof entry)[1]>] => entry[1] != null)
      .sort(([a, x], [b, y]) =>
        x.position < y.position ? -1 : x.position > y.position ? 1 : a < b ? -1 : 1,
      );
  }

  type Item = NonNullable<Service["items"][string]>;

  function itemTitle(item: Item): string {
    const k = item.kind;
    if ("Song" in k) return songInfo[k.Song.song]?.title ?? "Song";
    if ("Header" in k) return k.Header.title;
    if ("Scripture" in k) return formatPassage(k.Scripture.passage);
    if ("Deck" in k) return deckById[k.Deck.deck]?.title ?? "Presentation";
    if ("Media" in k) return k.Media.name;
    if ("Sound" in k) return k.Sound.sound.name;
    if ("Countdown" in k) return k.Countdown.label;
    if ("Text" in k) return textTitle(k.Text.title, k.Text.body);
    return "Announcements";
  }

  function itemSubtitle(item: Item): string {
    const k = item.kind;
    const label = kindLabel[kindName(k)];
    if ("Song" in k) {
      const author = songInfo[k.Song.song]?.author;
      return author ? `${label} · ${author}` : label;
    }
    if ("Scripture" in k) {
      const t = abbreviation[k.Scripture.translation];
      return t ? `${label} · ${t}` : label;
    }
    if ("Deck" in k) {
      const deck = deckById[k.Deck.deck];
      return deck ? `${label} · ${deckSourceLabel(deck.source)}` : label;
    }
    if ("Text" in k) {
      const n = splitText(k.Text.body).length;
      return `${label} · ${n === 1 ? "1 slide" : `${n} slides`}`;
    }
    if ("Media" in k) return `${mediaKind(k.Media.name)}${k.Media.looped ? " · loops" : ""}`;
    if ("Sound" in k) return soundSubtitle(k.Sound.sound);
    return label;
  }

  let items = $derived(selected ? ordered(selected) : []);

  /** The order as sections see it: which rows are headers. */
  let rows: Row[] = $derived(items.map(([id, item]) => ({ id, header: "Header" in item.kind, title: itemTitle(item) })));

  /** The card whose options are open, and the header whose loop panel is. */
  let openCard: string | null = $state(null);
  let loopOpen: string | null = $state(null);
  /** The gap a new header is being named in, by the index it would take. */
  let naming: number | null = $state(null);
  let headerName = $state("");
  let nameInput: HTMLInputElement | undefined = $state();
  /** Rows a header being named would cover: from its gap to the next header. */
  let covering = $derived(naming === null ? 0 : coveredAt(rows, naming));

  /** What each item puts on screen in a loop, for working out a pass. */
  let estimates: Record<string, ItemEstimate> = $state({});
  let estimating = 0;

  /** Names offered for a new header. */
  const commonHeaders = [
    "Pre-service",
    "Greeting",
    "Announcements",
    "Praise and Worship",
    "Communion",
    "Offering",
    "Sermon",
    "Post-service",
  ];

  async function loadEstimates(service: Service) {
    const ticket = ++estimating;
    try {
      const list = await request({ area: "Service", request: { Estimate: service.id } }, "Estimate");
      if (ticket !== estimating) return;
      estimates = Object.fromEntries(list.map((e) => [e.item, e]));
    } catch {
      // Without an estimate the loop panel shows no lengths.
    }
  }

  /** One pass of the section that the header at `index` starts. */
  function passOf(index: number, seconds: number) {
    return estimatePass(rows.slice(index + 1, sectionEnd(rows, index)), estimates, seconds);
  }

  function openNaming(at: number) {
    naming = at;
    headerName = "";
  }

  async function addHeader() {
    const title = headerName.trim();
    const at = naming;
    if (!title || at === null || !selected) return;
    const before = new Set(Object.keys(selected.items));
    naming = null;
    if (!(await addItem({ Header: { title } }, at)) || !selected) return;
    const made = Object.keys(selected.items).find((id) => !before.has(id));
    // Pre- and post-service sections nearly always loop, so ask at once.
    if (made && /pre|post/i.test(title)) loopOpen = made;
  }

  /** A video that repeats by itself holds a loop for good: play it once. */
  async function playOnce(item: string) {
    if (!selected) return;
    try {
      const entry = selected.items[item];
      if (entry && "Media" in entry.kind) {
        selected = await request(
          { area: "Service", request: { SetMediaLooped: { service: selected.id, item, looped: false } } },
          "Service",
        );
      }
    } catch (e) {
      report(e);
    }
  }

  /** What the section being dragged over would cover, for the drop hint. */
  let dragHint = $derived.by(() => {
    if (!dragging || dropAt === null || !rows[dragging.from]?.header) return "";
    const rest = rows.filter((_, i) => i !== dragging!.from);
    const covered = coveredAt(rest, dropAt);
    return `Section starts here · covers ${covered} ${covered === 1 ? "card" : "cards"}`;
  });

  // Reordering: drag a row by its handle, or focus the handle and press
  // the up and down arrows. Pointer events rather than HTML drag and drop,
  // which the webview's own file-drop handling can swallow. Library rows
  // are carried in the same way (see Library.svelte) and land at the line.
  let list: HTMLOListElement | undefined = $state();
  let orderBody: HTMLElement | undefined = $state();
  let dragging: { id: string; from: number } | null = $state(null);
  let dropAt: number | null = $state(null);
  /** Where a library row carried over the order would go, by index. */
  let carryAt: number | null = $state(null);

  /** The slot at `y` among the rows, skipping row `skip` (the one dragged). */
  function slotFor(y: number, skip = -1): number {
    if (!list) return 0;
    const rows = [...list.querySelectorAll<HTMLElement>("[data-row]")];
    let slot = 0;
    rows.forEach((row, i) => {
      if (i === skip) return;
      const r = row.getBoundingClientRect();
      if (y > r.top + r.height / 2) slot++;
    });
    return slot;
  }

  function carryOver(x: number, y: number): boolean {
    const r = orderBody?.getBoundingClientRect();
    const over = !!selected && !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    carryAt = over ? slotFor(y) : null;
    return over;
  }

  async function carryEnd(kind: ItemKind | null): Promise<boolean> {
    const at = carryAt;
    carryAt = null;
    return kind != null && at != null ? addItem(kind, at) : false;
  }

  function startDrag(e: PointerEvent, id: string, from: number) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = { id, from };
    dropAt = from;
  }

  function dragMove(e: PointerEvent) {
    if (dragging) dropAt = slotFor(e.clientY, dragging.from);
  }

  function endDrag() {
    if (dragging && dropAt !== null && dropAt !== dragging.from) move(dragging.id, dropAt);
    dragging = null;
    dropAt = null;
  }

  function handleKey(e: KeyboardEvent, id: string, index: number) {
    if (e.key === "ArrowUp" && index > 0) {
      e.preventDefault();
      move(id, index - 1);
    } else if (e.key === "ArrowDown" && index < items.length - 1) {
      e.preventDefault();
      move(id, index + 1);
    }
  }

  /** Where the drop line shows: above row `i`, or after the last row. */
  function lineBefore(i: number): boolean {
    if (carryAt !== null) return carryAt === i;
    if (!dragging || dropAt === null || dropAt === dragging.from) return false;
    // `dropAt` counts rows other than the dragged one.
    const visual = dropAt >= dragging.from ? dropAt + 1 : dropAt;
    return visual === i;
  }

  // Refetch Bibles after an import finishes.
  $effect(() => {
    void live.songsVersion;
    loadTranslations();
  });

  $effect(() => {
    const service = selected;
    if (service) untrack(() => lookUpSongs(service));
  });

  // Read what the items put on screen when the order changes, but only for
  // a service that has a header to loop.
  $effect(() => {
    const service = selected;
    if (service && Object.values(service.items).some((i) => i && "Header" in i.kind)) {
      untrack(() => loadEstimates(service));
    }
  });

  // Which sound files are on this device, for a service that has sound; read
  // again when files arrive from another device.
  $effect(() => {
    const service = selected;
    void live.soundVersion;
    if (service && Object.values(service.items).some((i) => i && ("Sound" in i.kind || i.backing))) {
      untrack(() => loadSoundStatus(service));
    } else {
      soundStatus = [];
    }
  });

  // Refetch decks when one is added or synced in.
  $effect(() => {
    void live.decksVersion;
    loadDecks();
  });

  refresh();
  if (live.planning) select(live.planning);
</script>

{#snippet namingRow(at: number)}
  <li class="naming">
    <div class="naming-head">
      <span class="eyebrow gold">New header</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        aria-label="Header name"
        placeholder="Name this section"
        bind:this={nameInput}
        bind:value={headerName}
        onkeydown={onKeys(addHeader, () => (naming = null))}
        autofocus
      />
      <Button variant="primary" size="small" onclick={addHeader} disabled={!headerName.trim()}>Add</Button>
      <Button variant="ghost" size="small" icon="x" label="Cancel" onclick={() => (naming = null)} />
    </div>
    <div class="chips" role="group" aria-label="Common headers">
      <span class="muted">Common headers</span>
      {#each commonHeaders as name (name)}
        <button
          type="button"
          class="chip"
          onclick={() => {
            headerName = name;
            nameInput?.focus();
          }}>{name}</button
        >
      {/each}
    </div>
    <p class="muted covers">
      {#if coveredAt(rows, at) === 0}
        Nothing follows yet, so this section starts empty.
      {:else}
        The {coveredAt(rows, at)} {coveredAt(rows, at) === 1 ? "card" : "cards"} below, up to the next header, will sit under this header.
      {/if}
    </p>
  </li>
{/snippet}

{#snippet zonePicker(when: When)}
  <select class="zone" aria-label="Time zone" title="The time zone this service's time is in" bind:value={when.zone}>
    {#each timeZones(when.zone, deviceZone()) as zone (zone)}
      <option value={zone}>{zone.replaceAll("_", " ")}{zone === deviceZone() ? " (this device)" : ""}</option>
    {/each}
  </select>
{/snippet}

<div class="plan">
  <aside class="pane pane-side services" aria-label="Services">
    <div class="pane-head">
      <h2>Services</h2>
      <Button size="small" icon="plus" onclick={toggleCreate} pressed={creating}>New service</Button>
    </div>
    {#if creating}
      <form
        class="create"
        onsubmit={(e) => {
          e.preventDefault();
          create();
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input aria-label="New service title" bind:value={newTitle} onkeydown={onKeys(create)} autofocus />
        <div class="when">
          <input type="date" aria-label="Service date" bind:value={newWhen.date} onkeydown={onKeys(create)} />
          <input type="time" aria-label="Service time" bind:value={newWhen.time} onkeydown={onKeys(create)} />
        </div>
        {@render zonePicker(newWhen)}
        {#if services.length > 0}
          <select aria-label="Start from a previous service" bind:value={copyFrom} onchange={pickCopy}>
            <option value="">Start empty</option>
            {#each services as s (s.id)}
              <option value={s.id}>Start from {s.title} · {formatWhen(s.starts_at, s.time_zone)}</option>
            {/each}
          </select>
        {/if}
        <Button variant="primary" size="small" onclick={create}>Create</Button>
      </form>
    {/if}
    <ul class="plain pane-body">
      {#each shown as s (s.id)}
        {#if renaming === s.id}
          <li class="rename">
            <!-- svelte-ignore a11y_autofocus -->
            <input
              aria-label="Service name"
              bind:value={renameTitle}
              onkeydown={onKeys(saveRename, () => (renaming = null))}
              onblur={saveRename}
              autofocus
            />
          </li>
        {:else}
          <ListRow title={s.title} subtitle={`${formatWhen(s.starts_at, s.time_zone)} · ${s.items} ${s.items === 1 ? "item" : "items"}`} active={selected?.id === s.id} onclick={() => select(s.id)}>
            {#snippet details()}
              {#if live.loaded?.service === s.id}
                <span class="row-status"><span class="live-dot"></span>Open for live</span>
              {/if}
            {/snippet}
            {#snippet trailing()}
              {#if selected?.id === s.id}
                <span class="row-actions">
                  <Button variant="ghost" size="small" onclick={() => startRename(s)}>Rename</Button>
                  <Button variant="ghost" size="small" title="Start a new service from this one" onclick={() => startFrom(s.id)}>Duplicate</Button>
                  {#if live.loaded?.service === s.id}
                    <Button variant="ghost" size="small" title="Close this service: take it off air and put it away" onclick={() => takeOffAir(s.id)}>Close</Button>
                  {/if}
                  {#if confirmDelete === s.id}
                    <Button variant="danger" size="small" onclick={() => deleteService(s.id)}>Delete?</Button>
                  {:else}
                    <Button variant="ghost" size="small" icon="trash" label="Delete service" title="Delete service" onclick={() => deleteService(s.id)} />
                  {/if}
                </span>
              {/if}
            {/snippet}
          </ListRow>
        {/if}
      {:else}
        <li class="empty muted">No services yet.</li>
      {/each}
      {#if olderCount > 0 || showOlder}
        <li class="older">
          <Button variant="ghost" size="small" onclick={() => (showOlder = !showOlder)}>
            {showOlder ? "Hide older services" : `Show ${olderCount} older`}
          </Button>
        </li>
      {/if}
    </ul>
    {#if newer > 0}
      <Banner tone="warn">
        {newer === 1 ? "1 service was" : `${newer} services were`} saved by a newer Lampstand and can't be opened here. Update Lampstand to see {newer === 1 ? "it" : "them"}.
      </Banner>
    {/if}
    {#each damaged as id (id)}
      <Banner tone="warn">
        A service file is damaged and can't be shown.
        <Button variant="ghost" size="small" icon="trash" onclick={() => removeDamaged(id)}>Delete it</Button>
      </Banner>
    {/each}
  </aside>

  <section class="pane order" aria-labelledby="service-title">
    {#if selected}
      <div class="order-head">
        <div class="heading">
          <h1 id="service-title">{selected.title}</h1>
          {#if editWhen}
            <form
              class="reschedule"
              onsubmit={(e) => e.preventDefault()}
            >
              <input type="date" aria-label="Service date" bind:value={editWhen.date} onkeydown={onKeys(saveWhen, cancelWhen)} />
              <input type="time" aria-label="Service time" bind:value={editWhen.time} onkeydown={onKeys(saveWhen, cancelWhen)} />
              {@render zonePicker(editWhen)}
              <Button variant="primary" size="small" onclick={saveWhen}>Save</Button>
              <Button variant="ghost" size="small" onclick={() => reschedule(null)} disabled={selected.starts_at == null}>Clear date</Button>
              <Button variant="ghost" size="small" icon="x" label="Cancel" onclick={cancelWhen} />
                  </form>
          {:else}
            <p class="muted when-line">
              <span>
                {formatWhen(selected.starts_at, selected.time_zone)} · {items.length}
                {items.length === 1 ? "item" : "items"}{#if soundTotalLabel(selected)} · {soundTotalLabel(selected)}{/if}
              </span>
              <Button variant="ghost" size="small" icon="calendar" label="Change date and time" title="Change date and time" onclick={startReschedule} />
            </p>
          {/if}
        </div>
        <div class="actions">
          <label class="look" title="How songs, scripture and text look in this service. A screen's own theme still wins.">
            <span class="muted">Theme</span>
            <ThemePicker label="Theme for {selected.title}" value={selected.theme} onchange={setTheme} />
          </label>
          <Button icon="deck" onclick={pickDeck} disabled={busy}>Add deck</Button>
          <Button icon="media" onclick={pickMedia} disabled={busy}>Add media</Button>
          {#if offerSound(selected, live.settings?.use_sound)}
            <Button icon="sound" onclick={pickSound} disabled={busy}>{soundWords.addSound}</Button>
          {/if}
          <PresentAction label="Open for live" onopen={openService} disabled={items.length === 0} />
        </div>
      </div>

      <div class="order-body" bind:this={orderBody}>
        {#if items.length === 0}
          <div class="empty-order" class:target={carryAt !== null}>
            <p><strong>This service is empty</strong></p>
            <p class="muted">Add songs, scripture, decks and media from the library on the right.</p>
          </div>
        {:else}
          <ol class="plain items" bind:this={list} aria-label="Service order">
            {#each items as [id, item], i (id)}
              {#if naming === i}
                {@render namingRow(i)}
              {:else if !dragging}
                <li class="slot"><button type="button" class="slot-add" aria-label="Add a header here" onclick={() => openNaming(i)}>+ Header here</button></li>
              {/if}
              {#if "Header" in item.kind}
                {@const header = item.kind.Header}
                {@const inLoop = item.loop_seconds != null}
                <li data-row class="header" class:dragging={dragging?.id === id} class:line-before={lineBefore(i)}>
                  {#if lineBefore(i) && dragHint}<span class="drop-hint">{dragHint}</span>{/if}
                  <button
                    type="button"
                    class="grip"
                    aria-label="Move {header.title}. Drag, or use the up and down arrows."
                    onpointerdown={(e) => startDrag(e, id, i)}
                    onpointermove={dragMove}
                    onpointerup={endDrag}
                    onpointercancel={endDrag}
                    onkeydown={(e) => handleKey(e, id, i)}
                  >
                    <Icon name="grip" />
                  </button>
                  <span class="header-title">{header.title}</span>
                  <span class="muted header-count">{sectionSize(rows, i)} {sectionSize(rows, i) === 1 ? "item" : "items"}</span>
                  <span class="header-rule"></span>
                  {#if inLoop}
                    <button type="button" class="loop-tag" aria-expanded={loopOpen === id} onclick={() => (loopOpen = loopOpen === id ? null : id)}>
                      Loops · {item.loop_seconds} s · {passLabel(passOf(i, item.loop_seconds ?? 0))}
                    </button>
                  {:else}
                    <button type="button" class="link" aria-expanded={loopOpen === id} onclick={() => (loopOpen = loopOpen === id ? null : id)}>Loop section</button>
                  {/if}
                  <Button variant="ghost" size="small" icon="trash" label="Remove {header.title}" onclick={() => remove(id)} />
                </li>
                {#if loopOpen === id}
                  <li class="panel-row">
                    <LoopPanel
                      title={header.title}
                      seconds={item.loop_seconds}
                      pass={item.loop_seconds ? passOf(i, item.loop_seconds) : null}
                      onchange={(seconds) => setLoop(id, seconds)}
                      onplayonce={playOnce}
                    />
                  </li>
                {/if}
              {:else}
                {@const owner = sectionOf(rows, i)}
                {@const looping = owner >= 0 && items[owner][1].loop_seconds != null}
                {@const isOpen = openCard === id}
                {@const loops = (item.loop_seconds ?? 0) > 0}
                <li
                  data-row
                  class="item"
                  class:grouped={owner >= 0}
                  class:looping
                  class:covered={naming !== null && i >= naming && i < naming + covering}
                  class:dragging={dragging?.id === id}
                  class:line-before={lineBefore(i)}
                >
                  <div class="card" class:open={isOpen}>
                    <div class="card-main">
                      <button
                        type="button"
                        class="grip"
                        aria-label="Move {itemTitle(item)}. Drag, or use the up and down arrows."
                        onpointerdown={(e) => startDrag(e, id, i)}
                        onpointermove={dragMove}
                        onpointerup={endDrag}
                        onpointercancel={endDrag}
                        onkeydown={(e) => handleKey(e, id, i)}
                      >
                        <Icon name="grip" />
                      </button>
                      {#if "Media" in item.kind}
                        <MediaThumb blob={item.kind.Media.blob} name={item.kind.Media.name} icon={kindIcon[kindName(item.kind)]} />
                      {:else if "Deck" in item.kind}
                        <DeckThumb deck={item.kind.Deck.deck} version={deckVersion(item.kind.Deck.deck)} />
                      {:else}
                        <span class="kind"><Icon name={kindIcon[kindName(item.kind)]} label={kindLabel[kindName(item.kind)]} /></span>
                      {/if}
                      <button type="button" class="text card-toggle" aria-expanded={isOpen} onclick={() => (openCard = isOpen ? null : id)}>
                        <span class="title">{itemTitle(item)}</span>
                        <span class="subtitle">{itemSubtitle(item)}</span>
                      </button>
                      {#if looping && "Media" in item.kind && item.kind.Media.looped}
                        <span class="tag warn">Loops by itself</span>
                      {/if}
                      {#if "Song" in item.kind && item.kind.Song.arrangement}<span class="tag">Custom order</span>{/if}
                      {#if item.backing}<span class="tag">{soundWords.backingTag}</span>{/if}
                      {#if "Sound" in item.kind && item.kind.Sound.sound.keep_off_stream || item.backing?.keep_off_stream}
                        <span class="tag">{soundWords.keptOffStreamTag}</span>
                      {/if}
                      {#if ("Sound" in item.kind && isMissing(soundStatus, id, false)) || (item.backing && isMissing(soundStatus, id, true))}
                        <span class="tag warn"><Icon name="alert" size={12} /> {soundWords.missingTag}</span>
                      {/if}
                      {#if loops}<span class="tag">Loops {item.loop_seconds} s</span>{/if}
                      {#if item.obs_scene}<span class="tag">OBS: {item.obs_scene}</span>{/if}
                      <button
                        type="button"
                        class="chevron"
                        class:turned={isOpen}
                        aria-label="{isOpen ? 'Hide' : 'Show'} options for {itemTitle(item)}"
                        aria-expanded={isOpen}
                        onclick={() => (openCard = isOpen ? null : id)}
                      >
                        <Icon name="next" size={14} />
                      </button>
                    </div>
                    {#if isOpen}
                      <div class="options">
                        {#if "Song" in item.kind && songInfo[item.kind.Song.song]?.sections}
                          {@const songId = item.kind.Song.song}
                          <div class="option">
                            <span class="eyebrow">Sequence for this service</span>
                            <SongOrder
                              sections={songInfo[songId].sections ?? []}
                              songOrder={songInfo[songId].order ?? []}
                              value={item.kind.Song.arrangement}
                              label="Order for {itemTitle(item)}"
                              onchange={(order) => setOrder(id, order)}
                              onsave={(order) => saveSongOrder(id, songId, order)}
                            />
                          </div>
                        {/if}
                        {#if "Sound" in item.kind}
                          <div class="option">
                            <SoundEditor
                              sound={item.kind.Sound.sound}
                              label={itemTitle(item)}
                              missing={isMissing(soundStatus, id, false)}
                              coming={isComing(soundStatus, id, false)}
                              onchange={(sound) => setSound(id, "Item", sound)}
                            />
                          </div>
                        {/if}
                        {#if "Song" in item.kind && item.backing}
                          <div class="option">
                            <span class="eyebrow">{soundWords.backing}</span>
                            <SoundEditor
                              sound={item.backing}
                              label="{soundWords.backing} of {itemTitle(item)}"
                              missing={isMissing(soundStatus, id, true)}
                              coming={isComing(soundStatus, id, true)}
                              onchange={(sound) => setSound(id, "Backing", sound)}
                            />
                            {#if confirmBacking === id}
                              <Button variant="ghost" size="small" icon="trash" onclick={() => { confirmBacking = null; setSound(id, "Backing", null); }}>{soundWords.removeBackingSure}</Button>
                              <Button variant="ghost" size="small" onclick={() => (confirmBacking = null)}>Keep it</Button>
                            {:else}
                              <Button variant="ghost" size="small" icon="trash" onclick={() => (confirmBacking = id)}>{soundWords.removeBacking}</Button>
                            {/if}
                          </div>
                        {:else if "Song" in item.kind && offerSound(selected, live.settings?.use_sound)}
                          <div class="option">
                            <Button size="small" icon="sound" onclick={() => addBacking(id)} disabled={busy}>{soundWords.addBacking}</Button>
                          </div>
                        {/if}
                        {#if "Text" in item.kind}
                          {@const text = item.kind.Text}
                          <div class="option">
                            <TextEditor
                              title={text.title}
                              body={text.body}
                              attribution={text.attribution}
                              label={itemTitle(item)}
                              onsave={(t, b, a) => setText(id, t, b, a)}
                            />
                          </div>
                        {/if}
                        <div class="option row-options">
                          {#if !("Sound" in item.kind)}
                            <LoopPicker
                              value={item.loop_seconds}
                              section={false}
                              label="Loop {itemTitle(item)}"
                              onchange={(seconds) => setLoop(id, seconds)}
                            />
                          {/if}
                          <!-- Only where OBS matters; a scene already set always shows. -->
                          {#if live.obs?.relevant || item.obs_scene}
                            <ObsScenePicker
                              value={item.obs_scene}
                              scenes={live.obs?.scenes ?? []}
                              label="OBS scene for {itemTitle(item)}"
                              onchange={(scene) => setObsScene(id, scene)}
                            />
                          {/if}
                          <span class="spacer"></span>
                          <Button variant="ghost" size="small" icon="trash" onclick={() => remove(id)}>Remove from service</Button>
                        </div>
                      </div>
                    {/if}
                  </div>
                </li>
              {/if}
            {/each}
            {#if naming === items.length}
              {@render namingRow(items.length)}
            {:else if !dragging}
              <li class="slot">
                <button type="button" class="slot-add" aria-label="Add a header at the end" onclick={() => openNaming(items.length)}>+ Header here</button>
                <button type="button" class="slot-add" onclick={addText}>+ Text slide</button>
              </li>
            {/if}
            {#if lineBefore(items.length)}<li class="line-end" aria-hidden="true">{#if dragHint}<span class="drop-hint">{dragHint}</span>{/if}</li>{/if}
          </ol>
        {/if}
      </div>
    {:else}
      <div class="no-service">
        <Icon name="calendar" size={24} />
        <p class="muted">Pick a service on the left, or create a new one.</p>
      </div>
    {/if}
  </section>

  <Library
    service={selected}
    {items}
    {decks}
    {translations}
    onadd={(kind) => addItem(kind)}
    ondragover={carryOver}
    ondragend={carryEnd}
    onsongs={songsFound}
    ondecks={loadDecks}
    ontranslations={loadTranslations}
  />
</div>

<style>
  .plan {
    display: grid;
    grid-template-columns: 248px minmax(0, 1fr) 340px;
    height: 100%;
  }
  .services {
    border-right: 1px solid var(--line);
  }
  .create {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0 1rem 0.75rem;
  }
  .create input,
  .reschedule input {
    min-width: 0;
    padding: 0.3rem 0.5rem;
  }
  .when {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 6.5rem;
    gap: 0.4rem;
  }
  .zone {
    min-width: 0;
    padding: 0.3rem 0.5rem;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .reschedule .zone {
    max-width: 12rem;
  }
  .create > :global(button) {
    align-self: flex-end;
  }
  .row-actions {
    display: flex;
    gap: 0.25rem;
  }
  .look {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
  }
  .look select {
    max-width: 11rem;
    padding: 0.3rem 0.5rem;
  }
  .rename {
    padding: 0.5rem 0.75rem;
  }
  .rename input {
    width: 100%;
  }
  .older {
    padding: 0.5rem 0.75rem;
  }
  .row-status {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.3rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--live);
  }
  .empty {
    padding: 2rem 0.75rem;
    text-align: center;
    font-size: 0.875rem;
  }

  /* Service order */
  .order-head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    padding: 1.5rem 2rem 1.25rem;
  }
  .heading {
    min-width: 0;
    flex: 1 1 16rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .heading p {
    margin-top: 0.25rem;
    font-size: 0.875rem;
  }
  .when-line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .reschedule {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.4rem;
  }
  .reschedule input {
    font-size: 0.85rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    /* Wrap inside the pane rather than run on under the Library. */
    max-width: 100%;
    min-width: 0;
  }
  .order-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 1rem 2rem 2rem;
  }
  .items {
    display: flex;
    flex-direction: column;
  }
  .item {
    position: relative;
    margin: 0.2rem 0;
  }
  /* Items under a header sit inset on a rail; a looping section's is gold. */
  .item.grouped {
    margin-left: 0.9rem;
    padding-left: 1.1rem;
    border-left: 2px solid var(--line-strong);
  }
  .item.grouped.looping {
    border-left-color: var(--gold);
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .card.open {
    border-color: var(--gold);
  }
  .item.covered .card {
    background: var(--raised);
    border-color: var(--line-strong);
  }
  .item.covered {
    margin-left: 0.9rem;
    padding-left: 1.1rem;
    border-left: 2px solid var(--gold);
  }
  .card-main {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 0.75rem 0.6rem 0.5rem;
  }
  .card-toggle {
    font: inherit;
    color: inherit;
    text-align: left;
    background: none;
    border: none;
    padding: 0.2rem 0;
    cursor: pointer;
  }
  .tag {
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--gold);
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    padding: 0.1rem 0.6rem;
    white-space: nowrap;
  }
  .tag.warn {
    color: var(--warn);
  }
  .chevron {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 2rem;
    height: 2rem;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
    transform: rotate(90deg);
  }
  .chevron.turned {
    transform: rotate(-90deg);
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
    padding: 0.9rem 1rem 1rem 5.25rem;
    border-top: 1px solid var(--line);
    background: var(--canvas);
  }
  .option {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .row-options {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 1rem;
  }
  .spacer {
    flex: 1;
  }
  .eyebrow {
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .eyebrow.gold {
    color: var(--gold);
  }

  /* A header is a slim label, not a card. */
  .header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.9rem;
    padding: 0.2rem 0.25rem 0.2rem 0;
  }
  .header-title {
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--gold);
  }
  .header-count {
    font-size: 0.75rem;
  }
  .header-rule {
    flex: 1;
    height: 1px;
    background: var(--line);
  }
  .loop-tag {
    font: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--gold);
    background: var(--gold-soft);
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    padding: 0.15rem 0.75rem;
    cursor: pointer;
  }
  .link {
    font: inherit;
    font-size: 0.75rem;
    color: var(--muted);
    background: none;
    border: none;
    text-decoration: underline;
    cursor: pointer;
  }
  .panel-row {
    margin: 0.25rem 0 0.5rem 0.9rem;
  }

  /* The gap between two cards: a gold line and "+ Header here" on hover. */
  .slot {
    position: relative;
    height: 0.6rem;
    display: flex;
    align-items: center;
  }
  .slot::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    border-radius: 2px;
    background: var(--gold);
    opacity: 0;
  }
  .slot-add {
    position: relative;
    margin-left: 0.5rem;
    font: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--gold);
    background: var(--canvas);
    border: 1px solid var(--gold);
    border-radius: 999px;
    padding: 0.05rem 0.75rem;
    opacity: 0;
    cursor: pointer;
  }
  .slot:hover::before,
  .slot:hover .slot-add,
  .slot:focus-within::before,
  .slot:focus-within .slot-add {
    opacity: 1;
  }
  .naming {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    margin: 0.4rem 0;
    padding: 0.9rem 1rem;
    background: var(--panel);
    border: 1px solid var(--gold);
    border-radius: var(--radius-lg);
  }
  .naming-head {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  .naming-head input {
    flex: 1;
    min-width: 0;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.8125rem;
  }
  .chip {
    font: inherit;
    font-size: 0.8125rem;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    padding: 0.15rem 0.75rem;
    cursor: pointer;
  }
  .chip:hover {
    border-color: var(--gold);
  }
  .covers {
    font-size: 0.8125rem;
  }
  .drop-hint {
    position: absolute;
    left: 0.5rem;
    top: -1.5rem;
    z-index: 1;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--on-gold);
    background: var(--gold);
    border-radius: 999px;
    padding: 0.05rem 0.7rem;
  }
  .item.dragging {
    opacity: 0.5;
  }
  .item.line-before::before,
  .line-end {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: -1px;
    height: 2px;
    background: var(--gold);
    border-radius: 2px;
  }
  .line-end {
    position: relative;
    display: block;
  }
  .grip {
    background: none;
    border: none;
    color: var(--faint);
    width: 1.25rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    cursor: grab;
    touch-action: none;
  }
  .grip:hover {
    color: var(--muted);
  }
  .grip:active {
    cursor: grabbing;
  }
  .kind {
    width: var(--tile-width);
    height: var(--tile-height);
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title {
    font-size: 0.9rem;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    font-size: 0.75rem;
    color: var(--muted);
  }
  .empty-order {
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
    padding: 4rem 1.5rem;
    text-align: center;
    font-size: 0.875rem;
  }
  .empty-order.target {
    border-color: var(--gold);
  }
  .empty-order p + p {
    margin-top: 0.25rem;
  }
  .no-service {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--faint);
    font-size: 0.875rem;
  }
</style>
