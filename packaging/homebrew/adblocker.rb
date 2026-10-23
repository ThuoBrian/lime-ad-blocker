# Homebrew formula template for a personal tap (e.g. ThuoBrian/homebrew-tap).
# After each release, set `version` and the two sha256 values from the
# `.sha256` files attached to the GitHub Release. See RELEASING.md.
class Adblocker < Formula
  desc "Lime: hosts-file ad blocker for macOS, Windows, and Linux"
  homepage "https://github.com/ThuoBrian/lime-ad-blocker"
  version "0.1.0"
  license "MIT"

  on_arm do
    url "https://github.com/ThuoBrian/lime-ad-blocker/releases/download/v#{version}/adblocker-v#{version}-aarch64-apple-darwin.tar.gz"
    sha256 "REPLACE_WITH_AARCH64_SHA256"
  end

  on_intel do
    url "https://github.com/ThuoBrian/lime-ad-blocker/releases/download/v#{version}/adblocker-v#{version}-x86_64-apple-darwin.tar.gz"
    sha256 "REPLACE_WITH_X86_64_SHA256"
  end

  def install
    bin.install "adblocker"
    pkgshare.install "scripts", "config.example.toml"
  end

  def caveats
    <<~EOS
      Download the blocklist, then write it to /etc/hosts:
        adblocker update
        sudo adblocker apply
      Optional daily refresh:
        sh #{opt_pkgshare}/scripts/install-schedule.sh
    EOS
  end

  test do
    assert_match "adblocker", shell_output("#{bin}/adblocker --version")
  end
end
