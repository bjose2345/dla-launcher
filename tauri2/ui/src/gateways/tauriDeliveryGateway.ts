import type {
  DeliveryGateway,
  DeliveryProgress,
  DeliveryView,
} from "@dla-launcher/shared-ui/delivery";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const tauriDeliveryGateway: DeliveryGateway = {
  read: () => invoke("read_delivery"),
  signIn: (username, password) => invoke("delivery_sign_in", { username, password }),
  signOut: () => invoke("delivery_sign_out"),
  install: (workCode) => invoke("delivery_install", { workCode }),
  pause: (jobId) => invoke("delivery_pause", { jobId }),
  resume: (jobId) => invoke("delivery_resume", { jobId }),
  cancel: (jobId) => invoke("delivery_cancel", { jobId }),
  remove: (jobId) => invoke("delivery_remove", { jobId }),
  chooseInstallFolder: () => invoke("select_delivery_install_folder"),
  subscribe(events) {
    const unlisteners: Promise<UnlistenFn>[] = [
      listen<DeliveryView>("delivery-changed", (event) => events.onChange(event.payload)),
      listen<DeliveryProgress>("delivery-progress", (event) => events.onProgress(event.payload)),
    ];
    return () => {
      for (const unlisten of unlisteners) void unlisten.then((stop) => stop());
    };
  },
};
