#!/usr/bin/env bash
# Rules every tracked file follows (CI job `repo-rules`; run it locally the same way):
#   1. No Hangul, except in the values of locales/ko.toml (keys and comments stay English).
#   2. No name of the project this one took its infrastructure from (written below in two
#      parts, so this file does not trip the rule itself).
#   3. No absolute paths of someone's home directory (`/Users/<name>`, `C:\Users\<name>`).
# Binary files are skipped. Prints every offending line and fails when there is one.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

git ls-files -z | perl -CS -e '
use strict;
use warnings;
my $old = "data" . "rig";
my $hangul = qr/[\x{1100}-\x{11FF}\x{3130}-\x{318F}\x{A960}-\x{A97F}\x{AC00}-\x{D7FF}]/;
my $failed = 0;
local $/ = "\0";
while (my $file = <STDIN>) {
    chomp $file;
    next if -d $file || -B $file;
    open(my $fh, "<:encoding(UTF-8)", $file) or die "$file: $!\n";
    local $/ = "\n";
    while (my $line = <$fh>) {
        my @why;
        if ($line =~ $hangul) {
            my $value_line = $line =~ /^\s*"[a-z0-9._]+"\s*=/ && $line !~ /^\s*#/;
            push @why, "Hangul outside the values of locales/ko.toml"
                unless $file eq "locales/ko.toml" && $value_line;
        }
        push @why, "the old project name" if $line =~ /\Q$old\E/i;
        push @why, "an absolute home path" if $line =~ m{/Users/[A-Za-z]|[A-Za-z]:\\{1,2}Users\\{1,2}[A-Za-z]}i;
        for my $w (@why) {
            print "$file:$.: $w\n";
            $failed = 1;
        }
    }
    close $fh;
}
exit $failed;
'
echo "repository rules: ok"
