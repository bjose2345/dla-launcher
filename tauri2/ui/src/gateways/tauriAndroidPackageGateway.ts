import type {
  AndroidAppGateway,
  AndroidAppView,
  AndroidPackageGateway,
  AndroidPackageState,
} from "@dla-launcher/shared-ui/android-package";
import { invoke } from "@tauri-apps/api/core";

export const tauriAndroidPackageGateway: AndroidPackageGateway = {
  readState(): Promise<AndroidPackageState> {
    return invoke("read_android_package_state");
  },
  selectAndInspect(): Promise<AndroidPackageState> {
    return invoke("select_and_inspect_android_package");
  },
  clearSelection(): Promise<AndroidPackageState> {
    return invoke("clear_android_package_selection");
  },
  openSourceApproval(): Promise<AndroidPackageState> {
    return invoke("open_android_package_source_approval");
  },
  requestInstall(): Promise<AndroidPackageState> {
    return invoke("request_android_package_install");
  },
};

export const tauriAndroidAppGateway: AndroidAppGateway = {
  list(): Promise<AndroidAppView[]> {
    return invoke("list_android_apps");
  },
  associateInstalled(workCode: string): Promise<AndroidAppView> {
    return invoke("associate_installed_android_app", { workCode });
  },
  launch(associationId: string): Promise<AndroidAppView> {
    return invoke("launch_android_app", { associationId });
  },
  remove(associationId: string): Promise<void> {
    return invoke("remove_android_app_association", { associationId });
  },
};
