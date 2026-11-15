/// <reference path="./kiln.d.ts" />

type Count = { value: number; history: number[] };

const valueLabel = document.getElementById("value")!;
const status = document.getElementById("status")!;
const inc = document.getElementById("inc")!;
const dec = document.getElementById("dec")!;
const reset = document.getElementById("reset")!;

function render(count: Count, note: string) {
  valueLabel.textContent = String(count.value);
  status.textContent = note;
}

async function shift(delta: number) {
  try {
    const count = await window.kiln.invoke<Count>("shift", { delta });
    render(count, `shift(${delta > 0 ? "+" : ""}${delta}) ok`);
  } catch (error) {
    status.textContent = "error: " + (error as Error).message;
  }
}

inc.addEventListener("click", () => shift(1));
dec.addEventListener("click", () => shift(-1));
reset.addEventListener("click", async () => {
  const count = await window.kiln.invoke<Count>("reset");
  render(count, "reset");
});

window.kiln.listen<Count>("count", (count) => render(count, "event from Rust"));

const initial = await window.kiln.invoke<Count>("current");
render(initial, "ready");
