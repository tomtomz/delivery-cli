include_recipe 'chef-sugar::default'

ruby_version = '2.1.6-x64'

execute "cargo clean" do
  if windows?
    environment({
      'PATH' => "C:/rubies/#{ruby_version}/bin;C:/rubies/#{ruby_version}/mingw/bin;C:/Program Files (x86)/Git/Cmd;C:/Program Files (x86)/Git/libexec/git-core;C:/wix;C:/7-zip;C:/Program Files (x86)/Windows Kits/8.1/bin/x64;C:/Windows/system32;C:/Windows;C:/Windows/System32/Wbem;C:/Program Files/OpenSSH/bin;C:/opscode/chef/bin/;C:/opscode/chefdk/bin/;C:/opscode/chefdk/embedded/mingw/bin;C:/Program Files/Rust nightly 1.4/bin;C:/chef/delivery-cli/bin;C:/chef/delivery-cli/bin"
    })
  end
  cwd node['delivery_builder']['repo']
end

if windows?
  execute "git config --global user.email \"delivery@chef.com\"" do
    cwd node['delivery_builder']['repo']
  end

  execute "git config --global user.name \"Delivery\"" do
    cwd node['delivery_builder']['repo']
  end
end

if windows?
  execute "cargo build" do
    if Chef::VERSION !~ /^12/
      environment({
      'RUST_TEST_TASKS' => "1"
      })
    end
    environment({
      'PATH' => "C:/rubies/#{ruby_version}/bin;C:/rubies/#{ruby_version}/mingw/bin;C:/Program Files (x86)/Git/Cmd;C:/Program Files (x86)/Git/libexec/git-core;C:/wix;C:/7-zip;C:/Program Files (x86)/Windows Kits/8.1/bin/x64;C:/Windows/system32;C:/Windows;C:/Windows/System32/Wbem;C:/Program Files/OpenSSH/bin;C:/opscode/chef/bin/;C:/opscode/chefdk/bin/;C:/opscode/chefdk/embedded/mingw/bin;C:/Program Files/Rust nightly 1.4/bin;C:/chef/delivery-cli/bin;C:/chef/delivery-cli/bin",
      'HOME' => ENV['USERPROFILE'],
      'HOMEDRIVE' => 'C:',
      'HOMEPATH' => '/Users/Administrator',
      'C_INCLUDE_PATH' => 'C:/OpenSSL-Win64/include;C:/opscode/chefdk/embedded/mingw/i686-w64-mingw32/include',
      'OPENSSL_INCLUDE_DIR' => 'C:/OpenSSL-Win64/include',
      'OPENSSL_LIB_DIR' => 'C:/OpenSSL-Win64',
      'LD_LIBRARY_PATH' => 'C:/OpenSSL-Win64',
      'SSL_CERT_FILE' => 'C:/rubies/#{ruby_version}/ssl/certs/cacert.pem'
    })
    cwd node['delivery_builder']['repo']
  end
else
  execute "cargo test" do
    if Chef::VERSION !~ /^12/
      environment({
        'RUST_TEST_TASKS' => "1"
      })
    end
    cwd node['delivery_builder']['repo']
  end
end 

if ! windows?
  execute "Cucumber Behavioral Tests" do
    command "make cucumber"
    cwd node['delivery_builder']['repo']
  end
end
