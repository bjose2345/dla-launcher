import type {
  ConfirmLibraryAddRequest,
  LibraryAddGateway,
  LibraryAddSource,
  LibraryAddView,
} from "@dla-launcher/shared-ui/library";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriLibraryAddGateway: LibraryAddGateway = {
  startLibraryAdd(source: LibraryAddSource): Promise<LibraryAddView | null> {
    return invoke("start_library_add", { request: { source } });
  },
  readLibraryAdd(): Promise<LibraryAddView | null> {
    return invoke("read_library_add");
  },
  confirmLibraryAdd(request: ConfirmLibraryAddRequest): Promise<LibraryAddView> {
    return invoke("confirm_library_add", { request });
  },
  cancelLibraryAdd(batchId: string): Promise<LibraryAddView | null> {
    return invoke("cancel_library_add", { batchId });
  },
  dismissLibraryAdd(batchId: string): Promise<void> {
    return invoke("dismiss_library_add", { batchId });
  },
  subscribeLibraryAdd(listener: (view: LibraryAddView) => void): Promise<() => void> {
    return listen<LibraryAddView>("library-add-changed", (event) => listener(event.payload));
  },
};
