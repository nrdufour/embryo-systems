#
# Makefile
#

VERSION=0.0.1

all:
	(cd src/orange; $(MAKE))
	(cd src/similar; $(MAKE))

doc:
	(cd src/orange; $(MAKE) doc)
	(cd src/similar; $(MAKE) doc)

test: all 
	(cd src/orange; $(MAKE) test)
	(cd src/similar; $(MAKE) test)

cover: all 
	(cd src/orange; $(MAKE) cover)
	(cd src/similar; $(MAKE) cover)

clean:
	(cd src/orange; $(MAKE) clean)
	(cd src/similar; $(MAKE) clean)

package: clean
	@mkdir embryosys-$(VERSION)/ && cp -rf Makefile TODO AUTHORS README src embryosys-$(VERSION)
	@COPYFILE_DISABLE=true tar zcf embryosys-$(VERSION).tgz embryosys-$(VERSION)
	@rm -rf embryosys-$(VERSION)/

install: all
	(cd src/orange; $(MAKE) install)
	(cd src/similar; $(MAKE) install)
	# FIXME Need to deploy the common files too here

