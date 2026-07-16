# Homebrew formula for LightSandbox server
# Published at: https://github.com/lipiji/homebrew-lightsandbox
#
# To install:
#   brew tap lipiji/lightsandbox
#   brew install lightsandbox
#
# This file is updated automatically by the release workflow.
# SHA256 checksums below must match the release artifacts.

class Lightsandbox < Formula
  desc "Self-hosted sandbox execution for AI agents"
  homepage "https://github.com/lipiji/LightSandbox"
  version "PLACEHOLDER_VERSION"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/lipiji/LightSandbox/releases/download/PLACEHOLDER_VERSION/lightsandbox-server-macos-arm64"
      sha256 "PLACEHOLDER_SHA256_MACOS_ARM64"

      def install
        bin.install "lightsandbox-server-macos-arm64" => "lightsandbox-server"
      end
    end

    on_intel do
      url "https://github.com/lipiji/LightSandbox/releases/download/PLACEHOLDER_VERSION/lightsandbox-server-macos-x86_64"
      sha256 "PLACEHOLDER_SHA256_MACOS_X86_64"

      def install
        bin.install "lightsandbox-server-macos-x86_64" => "lightsandbox-server"
      end
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/lipiji/LightSandbox/releases/download/PLACEHOLDER_VERSION/lightsandbox-server-linux-arm64"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"

      def install
        bin.install "lightsandbox-server-linux-arm64" => "lightsandbox-server"
      end
    end

    on_intel do
      url "https://github.com/lipiji/LightSandbox/releases/download/PLACEHOLDER_VERSION/lightsandbox-server-linux-x86_64"
      sha256 "PLACEHOLDER_SHA256_LINUX_X86_64"

      def install
        bin.install "lightsandbox-server-linux-x86_64" => "lightsandbox-server"
      end
    end
  end

  test do
    # start server in background, hit /health, then kill it
    port = free_port
    pid = spawn("#{bin}/lightsandbox-server", "--config", "/dev/null",
                env: { "RUST_LOG" => "error" })
    sleep 1
    assert_match "ok", shell_output("curl -s http://127.0.0.1:#{port}/health")
  ensure
    Process.kill("TERM", pid)
  end
end
