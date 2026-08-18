import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function SuppliersRoute() {
  return <GenericScreen config={screenConfig("suppliers")} />;
}
