**Nessun fix dell’audit applicato. Ti lascio il report prima di intervenire.** L’audit non è ancora chiuso: la copertura è terminata, ma manca il consolidamento indipendente dei rilievi di sicurezza.

### Quanto trovato

**Qualità — verificata sul sorgente**
- **Confini dei turni discordanti** tra riepiloghi e attività: possibile associazione incoerente dei contenuti.
- **I/O evitabile:** lettura completa dei transcript prima del cursore; la vista read-only del subagent riparte da zero durante il polling.
- **Logica duplicata nel bridge:** `stop_turn` pubblica direttamente un evento fuori dal normale sink. Non ho riprodotto un guasto UI.
- **Test MCP presenti ma non collegati alla CI** esaminata.
- Su **48 osservazioni**: 24 confermate sul sorgente, 23 corrette o ridimensionate, 1 smentita. **Non sono 48 bug riprodotti.**

**Sicurezza — candidati ancora da finalizzare**
- HTML non codificato nell’highlighter e isolamento delle anteprime.
- Endpoint MCP senza autenticazione; iframe delle opzioni sullo stesso origin dell’interfaccia di conferma.
- Script del workspace potenzialmente promossi nella configurazione MCP globale.
- Grant dei comandi confrontati per basename anziché percorso completo.
- Import skill Project attraverso symlink.
- Credenziali in argv, permessi dei file e possibili blocchi/consumi crescenti.

Alcune fixture hanno osservato comportamenti reali; altre verifiche restano solo sorgente o modelli equivalenti. **Non assegno severità definitive senza distinguere questi casi.**

### Prossime azioni
1. Completare le verifiche indipendenti e scartare le conferme non sostenute da prove sul target.
2. Consegnare la lista finale con impatto, priorità e fix minimi proposti.
3. Solo dopo il report, applicare i fix e verificarli.
4. Ricostruire l’AppImage se i fix modificano l’app. Quella della correzione **Thinking è già stata ricostruita**.

Report salvato: [REPORT.md](file:///home/GioViale/security-audit-skill/OhMyPiApp/run-1/REPORT.md). Registro di copertura validato: **60 unità**.

Le fixture dell’audit sono state eseguite **in serie, su un solo core, con quota CPU 25% e RAM massima 512 MiB**.

Non verificati: effetti nel WebView Tauri, accesso tra utenti OS e copia MCP installata. Il report è esplicitamente intermedio, non una certificazione di sicurezza.
