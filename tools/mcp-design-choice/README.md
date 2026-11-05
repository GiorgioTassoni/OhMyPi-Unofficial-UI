# MCP Design Choice & Preview (`mcp-design-choice`)

Server MCP universale Human-in-the-Loop per proporre varianti di design HTML/CSS all'utente, visualizzarle e attendere la scelta nello stesso turno dell'agente.

## Caratteristiche

- **Sincrono / Human-in-the-loop**: Quando l'agente chiama `design_choice`, il server avvia un mini web server HTTP su porta libera locale e **attende la selezione dell'utente prima di restituire il risultato**, permettendo all'agente di continuare nello stesso identico turn.
- **Universale**: Funziona con qualsiasi client MCP (OMP, OhMyPiApp, Claude Desktop, Cursor, terminale).
- **UI Web Standalone inclusa**:
  - Tab per confrontare istantaneamente le varianti HTML.
  - Selettore di viewport (Full / Tablet 768px / Mobile 375px).
  - Toggle Anteprima renderizzata (iframe isolato) vs Codice sorgente HTML.
  - Campo note/feedback per l'utente.
  - Pulsante per confermare la scelta.
- **Integrazione Terminale / CLI**: Oltre alla UI web, l'utente può scegliere direttamente da terminale con una chiamata `curl`.

## Installazione & Esecuzione

```bash
cd tools/mcp-design-choice
bun install
```

## Configurazione in OMP (`.mcp.json`)

Crea o modifica il file `.mcp.json` nella root del progetto:

```json
{
  "mcpServers": {
    "design-choice": {
      "command": "bun",
      "args": ["tools/mcp-design-choice/src/index.ts"]
    }
  }
}
```

## Tool esposti

1. **`design_choice`**:
   - `title`: Titolo o contesto della scelta.
   - `description`: Note o spiegazioni delle differenze.
   - `options`: Array di opzioni (`id`, `label`, `html`, `css`, `notes`).
2. **`preview_html`**:
   - `title`: Titolo dell'anteprima.
   - `html`: Codice HTML/CSS da renderizzare.
