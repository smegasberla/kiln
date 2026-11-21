const log = document.getElementById("log");
let lines = [];
function write(label, value) {
  lines = [...lines.slice(-14), `${label}: ${JSON.stringify(value)}`];
  log.textContent = lines.join("\n");
}
async function run(label, method, payload) {
  const started = performance.now();
  try {
    const value = await voussoir.invoke(method, payload);
    write(`${label} (${Math.round(performance.now() - started)}ms)`, value);
  } catch (error) {
    write(`${label} FAILED`, error.message);
  }
}

document.getElementById("stats").onclick = () => run("stats", "stats");
document.getElementById("slow").onclick = () => run("slow", "slow_task", { ms: 800 });
document.getElementById("window").onclick = () => run("window", "open_settings");
document.getElementById("open").onclick = () => run("open", "dialog.open", { title: "Open a file" });
document.getElementById("save").onclick = () =>
  run("save", "dialog.save", { title: "Save as", file_name: "untitled.txt" });
document.getElementById("update").onclick = () => run("update", "update.check");
document.getElementById("devtools").onclick = () => run("devtools", "__voussoir_devtools");

voussoir.listen("menu", (event) => write("menu", event));
voussoir.listen("tray", (event) => write("tray", event));

run("version", "__voussoir_version");
