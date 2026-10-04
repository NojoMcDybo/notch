/**
 * Morphen statt neu bauen: neue Daten (Spielstand, Minute, Ticker …) aendern nur, was sich wirklich geaendert
 * hat. Der Aufrufer baut den neuen Inhalt wie gewohnt als frische Elemente; morph() gleicht ihn mit dem
 * bestehenden ab und behaelt alles Gleiche — Bilder laden nicht neu (kein Aufblinken der Wappen/Cover),
 * Animationen und Uebergaenge laufen weiter, nur neue Zeilen blenden ein.
 *
 * - Elemente mit `data-key` werden ueber den Schluessel zugeordnet (Spiele, Tickerzeilen, Tabellenzeilen …),
 *   die uebrigen der Reihe nach, solange Tag und Schluessel passen.
 * - `data-keep`: Element lebt selbst (Spielfeld, Wurfbild) — wird nur eingesetzt, nie angefasst.
 * - Die Glasperle der Segmente (`.n-seg-thumb`) bleibt stehen.
 * - Handler als Eigenschaft (`onclick = …`) werden uebernommen; mit addEventListener gesetzte bleiben am alten
 *   Element. Deshalb Elemente mit Klick einen `data-key` geben, der ihre Identitaet beschreibt.
 *
 * Identische Kopie in D:\Dev\arena\src\morph.ts und D:\Dev\notch\src\morph.ts — Aenderungen in beiden.
 */

/** die gleitende Glasperle der Segmente (nojo-ui segments()) gehoert nicht zum Inhalt: stehen lassen, damit sie gleitet */
const own = (o: Node) => o.nodeType === 1 && (o as Element).classList.contains("n-seg-thumb");

const keyOf = (n: Node): string | undefined => (n.nodeType === 1 ? (n as HTMLElement).dataset?.key || undefined : undefined);

function same(a: Node, b: Node) {
  if (a.nodeType !== b.nodeType) return false;
  if (a.nodeType !== 1) return true;
  const ea = a as Element, eb = b as Element;
  return ea.tagName === eb.tagName && ea.namespaceURI === eb.namespaceURI && keyOf(a) === keyOf(b);
}

/** Handler, die als Eigenschaft gesetzt werden und mitwandern (onerror/onload bewusst nicht: Bilder) */
const HANDLERS = ["onclick", "ondblclick", "onmousedown", "onpointerdown", "oncontextmenu", "onmouseenter", "onmouseleave", "onkeydown", "oninput", "onchange"] as const;

function patch(o: Node, n: Node) {
  if (o === n) return;
  if (o.nodeType !== 1) {
    if (o.nodeValue !== n.nodeValue) o.nodeValue = n.nodeValue;
    return;
  }
  const eo = o as HTMLElement, en = n as HTMLElement;
  if (eo.dataset.keep != null) return;
  for (const a of Array.from(eo.attributes)) if (!en.hasAttribute(a.name)) eo.removeAttribute(a.name);
  for (const a of Array.from(en.attributes)) if (eo.getAttribute(a.name) !== a.value) eo.setAttribute(a.name, a.value);
  const ro = eo as unknown as Record<string, unknown>, rn = en as unknown as Record<string, unknown>;
  for (const h of HANDLERS) if (ro[h] !== rn[h]) ro[h] = rn[h];
  // Eingabefelder behalten ihren Inhalt (der Nutzer tippt vielleicht gerade)
  if (eo instanceof HTMLInputElement || eo instanceof HTMLTextAreaElement || eo instanceof HTMLSelectElement) return;
  morph(eo, Array.from(en.childNodes));
}

/** Inhalt von `parent` auf `next` bringen und dabei so viel wie moeglich behalten */
export function morph(parent: Element, next: Node[]) {
  const old = Array.from(parent.childNodes);
  const keyed = new Map<string, Node>();
  for (const o of old) {
    const k = keyOf(o);
    if (k) keyed.set(k, o);
  }
  const used = new Set<Node>();
  const out: Node[] = [];
  let i = 0;
  for (const n of next) {
    const k = keyOf(n);
    let m: Node | undefined;
    if (k) {
      const o = keyed.get(k);
      if (o && !used.has(o) && same(o, n)) m = o;
    } else {
      while (i < old.length && (used.has(old[i]) || keyOf(old[i]) || own(old[i]))) i++;
      if (i < old.length && same(old[i], n)) m = old[i++];
    }
    if (m) {
      used.add(m);
      patch(m, n);
      out.push(m);
    } else out.push(n);
  }
  const keep = new Set(out);
  for (const o of old) if (!keep.has(o) && !own(o) && o.parentNode === parent) parent.removeChild(o);
  // in Reihenfolge bringen; nur bewegen, was nicht schon an seinem Platz steht
  let ref: ChildNode | null = parent.firstChild;
  for (const n of out) {
    while (ref && own(ref)) ref = ref.nextSibling;
    if (n === ref) {
      ref = ref.nextSibling;
      continue;
    }
    parent.insertBefore(n, ref);
  }
}
