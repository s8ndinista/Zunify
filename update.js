if ("serviceWorker" in navigator) {
  const workers = navigator.serviceWorker;
  const update = document.querySelector("#app-update");
  let controller = workers.controller;

  update.querySelector("button").addEventListener("click", () => location.reload());
  workers.addEventListener("controllerchange", () => {
    if (controller && controller !== workers.controller) update.hidden = false;
    controller = workers.controller;
  });
  workers.register("./sw.js", { updateViaCache: "none" }).then(registration => {
    const checkForUpdate = () => {
      if (!document.hidden) registration.update().catch(() => {});
    };
    document.addEventListener("visibilitychange", checkForUpdate);
    window.addEventListener("pageshow", event => {
      if (event.persisted) checkForUpdate();
    });
    setInterval(checkForUpdate, 5 * 60 * 1000);
  }).catch(() => {});
}
