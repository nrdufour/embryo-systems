/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.manager;

/**
 * @author nrdufour
 *
 */
public interface Repository {

	Session login();
	
	Session login(String workspaceName);
}
