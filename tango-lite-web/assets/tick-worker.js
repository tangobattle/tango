// A heartbeat that a hidden tab can't throttle away.
//
// `requestAnimationFrame` stops entirely in a background tab, and
// `setInterval` on the main thread is clamped to about once a second.
// Timers inside a dedicated worker are not clamped that way, so this
// posts a bare message at roughly the frame rate and the main thread
// pumps the session on receipt.
//
// It matters for netplay specifically: a stalled simulation isn't a
// local inconvenience, it backs the peer's input queue up until their
// supervisor decides the link is dead. Without this, backgrounding the
// tab puts the match into an endless reconnect loop.

let timer = null;

onmessage = (e) => {
    if (e.data === "start") {
        if (timer === null) {
            // Faster than 60Hz on purpose: the main thread's pump
            // advances by elapsed wall clock, so an early tick costs
            // one cheap no-op call and a late one costs a visible
            // hitch.
            timer = setInterval(() => postMessage(0), 8);
        }
    } else if (e.data === "stop") {
        clearInterval(timer);
        timer = null;
    }
};
