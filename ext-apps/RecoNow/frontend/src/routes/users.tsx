import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function UsersRoute() {
  return <GenericScreen config={screenConfig("users")} />;
}
