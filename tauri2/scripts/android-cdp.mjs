const [port, expression] = process.argv.slice(2);

if (!port || !expression) {
  console.error("usage: node android-cdp.mjs <port> <expression>");
  process.exit(2);
}

const pages = await fetch(`http://127.0.0.1:${port}/json`).then((response) => {
  if (!response.ok) throw new Error(`CDP page list returned ${response.status}`);
  return response.json();
});
const page = pages.find((candidate) => candidate.type === "page") ?? pages[0];
if (!page?.webSocketDebuggerUrl) throw new Error("no debuggable WebView page is available");

const socket = new WebSocket(page.webSocketDebuggerUrl);
const timeout = setTimeout(() => {
  console.error("CDP evaluation timed out");
  socket.close();
  process.exit(1);
}, 10_000);

socket.addEventListener("open", () => {
  socket.send(JSON.stringify({
    id: 1,
    method: "Runtime.evaluate",
    params: {
      expression,
      awaitPromise: true,
      returnByValue: true,
    },
  }));
});

socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (message.id !== 1) return;
  clearTimeout(timeout);
  if (message.result?.exceptionDetails) {
    console.error(message.result.exceptionDetails.text ?? "CDP evaluation failed");
    socket.close();
    process.exit(1);
  }
  const value = message.result?.result?.value;
  if (typeof value === "object") console.log(JSON.stringify(value));
  else if (value !== undefined) console.log(String(value));
  socket.close();
});

socket.addEventListener("error", () => {
  clearTimeout(timeout);
  console.error("CDP WebSocket connection failed");
  process.exit(1);
});
