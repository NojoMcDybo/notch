// CSS-Dateien als Seiteneffekt-Import (Schriften, esports.css)
declare module "*.css";

// Vite: Datei als Adresse importieren (z. B. die WASM-Laufzeit fuer „Hey Jarvis“)
declare module "*?url" {
  const url: string;
  export default url;
}
