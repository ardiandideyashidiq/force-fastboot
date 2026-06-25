import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DeviceInfo {
    connected: boolean;
    serial: string | null;
    vars: Record<string, string>;
}

function App() {
    const [device, setDevice] = useState<DeviceInfo | null>(null);
    const [loading, setLoading] = useState(false);

    async function fetchDevice() {
        setLoading(true);
        try {
            const info = await invoke<DeviceInfo>("get_device_info");
            setDevice(info);
        } catch (e) {
            console.error("get_device_info failed", e);
        } finally {
            setLoading(false);
        }
    }

    useEffect(() => {
        fetchDevice();
    }, []);

    return (
        <main>
            <h1>pawflash</h1>
            <p>MTK device flashing toolkit</p>

            <section>
                <h2>Device</h2>
                {loading && <p>Connecting...</p>}
                {device && (
                    <pre>{JSON.stringify(device, null, 2)}</pre>
                )}
                <button onClick={fetchDevice} disabled={loading}>
                    Refresh
                </button>
            </section>
        </main>
    );
}

export default App;
