const result = document.getElementById("result");
const tick = document.getElementById("tick");
const form = document.getElementById("greet");
const name = document.getElementById("name");

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  result.textContent = "invoking...";
  try {
    const greeting = await voussoir.invoke("greet", { name: name.value });
    result.textContent = greeting;
    voussoir.invoke("report", { greeting }).catch(() => {});
  } catch (error) {
    result.textContent = "error: " + error.message;
  }
});

voussoir.listen("tick", (n) => {
  tick.textContent = "rust emitted event #" + n;
});

form.requestSubmit();
