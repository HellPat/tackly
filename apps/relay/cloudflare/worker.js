// The relay on Cloudflare: this Worker forwards every request to one container
// running the relay binary (Dockerfile). One instance, because the relay
// allocates event sequence numbers with a single SQLite writer and fans live
// events out to the SSE streams it holds in memory.
import { Container } from "@cloudflare/containers";

export class Relay extends Container {
  defaultPort = 3000;
  // An open live stream counts as activity, so this only stops a relay nobody
  // is connected to; the next request restores the database from R2.
  sleepAfter = "30m";

  constructor(ctx, env) {
    super(ctx, env);
    this.envVars = {
      R2_ACCOUNT_ID: env.R2_ACCOUNT_ID,
      R2_BUCKET: env.R2_BUCKET,
      R2_ACCESS_KEY_ID: env.R2_ACCESS_KEY_ID ?? "",
      R2_SECRET_ACCESS_KEY: env.R2_SECRET_ACCESS_KEY ?? "",
    };
  }
}

export default {
  async fetch(request, env) {
    return env.RELAY.getByName("relay").fetch(request);
  },
};
